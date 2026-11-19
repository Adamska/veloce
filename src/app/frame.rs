use std::time::{Duration, Instant};

use winit::event_loop::ActiveEventLoop;

use super::viewport::Viewport;
use crate::{
    gpu::Placement,
    ui::{RenderStats, ViewContext, theme},
};

const OCCLUDED_BACKOFF: Duration = Duration::from_millis(8);

impl Viewport {
    fn video_rect(&self, placement: &Placement) -> egui::Rect {
        let (width, height) = self.gpu.size();
        let scale = self.window.scale_factor() as f32;
        let screen = egui::vec2(width as f32 / scale, height as f32 / scale);
        let center = egui::pos2(
            (placement.offset[0] + 1.0) / 2.0 * screen.x,
            (placement.offset[1] + 1.0) / 2.0 * screen.y,
        );
        let size = egui::vec2(placement.scale[0] * screen.x, placement.scale[1] * screen.y);
        egui::Rect::from_center_size(center, size)
            .intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, screen))
    }

    pub fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame).as_secs_f32() * 1_000.0;
        self.frame_millis += (elapsed.min(250.0) - self.frame_millis) * 0.1;
        self.last_frame = now;

        self.handle_drops();
        for result in self.snapshots.1.try_iter().collect::<Vec<_>>() {
            match result {
                Ok(path) => self
                    .player
                    .notify(format!("Snapshot saved to {}", path.display())),
                Err(error) => self
                    .player
                    .report_error(format!("Snapshot failed: {error:#}")),
            }
        }

        let selection = self.player.update(self.refresh_interval());
        let (width, height) = self.gpu.size();
        let mut video_rect = None;
        match &selection {
            Some(selection) => {
                let placement = self.player.geometry.placement(
                    selection.primary.display_aspect(),
                    [width as f32, height as f32],
                );
                self.video.prepare(
                    &self.gpu.device,
                    &self.gpu.queue,
                    selection,
                    &self.player.adjustments,
                    &placement,
                );
                video_rect = Some(self.video_rect(&placement));
            }
            None if self.player.info().is_none() => self.video.clear(),
            None => {}
        }

        let notices = self.player.take_notices();
        self.interface.notify(notices, self.egui.context());
        self.sync_window();

        let mut actions = Vec::new();
        let view = ViewContext {
            player: &self.player,
            displayed: self.video.displayed().map(|frame| &**frame),
            video_rect: video_rect.filter(|_| self.video.displayed().is_some()),
            stats: RenderStats {
                adapter: &self.gpu.adapter_name,
                fps: 1_000.0 / self.frame_millis.max(0.01),
                frame_millis: self.frame_millis,
                uploads: self.video.uploads,
            },
        };
        let interface = &mut self.interface;
        let frame = self
            .egui
            .run(&self.window, &self.gpu.device, &self.gpu.queue, |ui| {
                interface.show(ui, &view, &mut actions);
            });

        let animating = self.player.has_session() && !self.player.is_paused();
        self.next_repaint = if animating || frame.repaint_after.is_zero() {
            Some(now)
        } else {
            now.checked_add(frame.repaint_after)
        };

        let Some(surface) = self.gpu.acquire() else {
            self.egui.finish(frame);
            std::thread::sleep(OCCLUDED_BACKOFF);
            return;
        };
        let target = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.egui
            .prepare(&self.gpu.device, &self.gpu.queue, &mut encoder, &frame);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frame"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(theme::CLEAR_COLOR),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.video.draw(&mut pass);
            self.egui.paint(&mut pass, &frame);
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.gpu.queue.present(surface);
        self.egui.finish(frame);

        for action in actions {
            self.dispatch(action, event_loop);
        }
    }
}
