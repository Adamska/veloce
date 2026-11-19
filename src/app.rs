use std::{ffi::OsString, sync::Arc};

use anyhow::Result;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

use crate::{
    gpu::GpuContext,
    ui::{EguiLayer, theme},
};

pub fn run(_arguments: Vec<OsString>) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct Viewport {
    window: Arc<Window>,
    gpu: GpuContext,
    egui: EguiLayer,
}

#[derive(Default)]
struct App {
    viewport: Option<Viewport>,
}

impl Viewport {
    fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("Veloce")
            .with_inner_size(LogicalSize::new(1280.0, 720.0))
            .with_min_inner_size(LogicalSize::new(640.0, 360.0));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let gpu = GpuContext::new(window.clone())?;
        let egui = EguiLayer::new(&window, &gpu.device, gpu.format());
        Ok(Self { window, gpu, egui })
    }

    fn redraw(&mut self) {
        let frame = self
            .egui
            .run(&self.window, &self.gpu.device, &self.gpu.queue, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("Veloce")
                            .font(theme::sans(34.0))
                            .color(theme::TEXT),
                    );
                });
            });
        let Some(surface) = self.gpu.acquire() else {
            self.egui.finish(frame);
            return;
        };
        let view = surface.texture.create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.egui
            .prepare(&self.gpu.device, &self.gpu.queue, &mut encoder, &frame);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frame"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
            self.egui.paint(&mut pass, &frame);
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.gpu.queue.present(surface);
        self.egui.finish(frame);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.viewport.is_some() {
            return;
        }
        match Viewport::new(event_loop) {
            Ok(viewport) => self.viewport = Some(viewport),
            Err(error) => {
                log::error!("failed to create the main window: {error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(viewport) = self.viewport.as_mut() else {
            return;
        };
        if viewport
            .egui
            .on_window_event(&viewport.window, &event)
            .repaint
        {
            viewport.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                viewport.gpu.resize(size.width, size.height);
                viewport.window.request_redraw();
            }
            WindowEvent::RedrawRequested => viewport.redraw(),
            _ => {}
        }
    }
}
