use std::path::PathBuf;

use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use veloce::{
    gpu::{FrameSelection, VideoPipeline, snapshot::Snapshot},
    player::Player,
    ui::{Action, Interface, RenderStats, ViewContext, theme},
};

const DEFAULT_SIZE: [u32; 2] = [1920, 1080];
const SCALE: f32 = 1.5;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Stage {
    size: [u32; 2],
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub ctx: egui::Context,
    renderer: Renderer,
    video: VideoPipeline,
    target: wgpu::Texture,
    time: f64,
    /// Simulates a moving mouse so the control capsule stays visible during playback.
    pub pointer_moving: bool,
}

impl Stage {
    pub fn new() -> Option<Self> {
        Self::with_size(DEFAULT_SIZE)
    }

    pub fn with_size(size: [u32; 2]) -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let norm16 = adapter.features() & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: norm16,
            ..Default::default()
        }))
        .ok()?;
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui smoke"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Some(Self {
            size,
            renderer: Renderer::new(&device, FORMAT, RendererOptions::default()),
            video: VideoPipeline::new(&device, FORMAT),
            device,
            queue,
            ctx,
            target,
            time: 0.0,
            pointer_moving: false,
        })
    }

    pub fn frame(
        &mut self,
        interface: &mut Interface,
        player: &Player,
        selection: Option<&FrameSelection>,
    ) -> Vec<Action> {
        let screen = egui::vec2(self.size[0] as f32, self.size[1] as f32) / SCALE;
        let mut video_rect = None;
        if let Some(selection) = selection {
            let placement = player
                .geometry
                .placement(selection.primary.display_aspect(), [screen.x, screen.y]);
            self.video.prepare(
                &self.device,
                &self.queue,
                selection,
                &player.adjustments,
                &placement,
            );
            video_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen));
        }
        self.time += 1.0 / 30.0;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            time: Some(self.time),
            ..Default::default()
        };
        if self.pointer_moving {
            let wiggle = if ((self.time * 30.0) as u64).is_multiple_of(2) {
                0.0
            } else {
                1.0
            };
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(200.0 + wiggle, 200.0)));
        }
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(SCALE);
        let mut actions = Vec::new();
        let view = ViewContext {
            player,
            displayed: self.video.displayed().map(|frame| &**frame),
            video_rect,
            stats: RenderStats {
                adapter: "headless",
                fps: 120.0,
                frame_millis: 8.33,
                uploads: self.video.uploads,
            },
        };
        let mut output = self
            .ctx
            .run_ui(input, |ui| interface.show(ui, &view, &mut actions));
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        output.textures_delta.clear();
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let descriptor = ScreenDescriptor {
            size_in_pixels: self.size,
            pixels_per_point: output.pixels_per_point,
        };
        let view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &primitives,
            &descriptor,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ui smoke"),
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
            self.video.draw(&mut pass);
            self.renderer.render(&mut pass, &primitives, &descriptor);
        }
        self.queue.submit([encoder.finish()]);
        actions
    }

    pub fn settle(
        &mut self,
        interface: &mut Interface,
        player: &Player,
        selection: Option<&FrameSelection>,
    ) {
        for _ in 0..20 {
            let actions = self.frame(interface, player, selection);
            assert!(actions.is_empty(), "idle interface emitted {actions:?}");
        }
    }

    pub fn save(&self, name: &str) {
        let Some(directory) = std::env::var_os("VELOCE_UI_SNAPSHOT_DIR").map(PathBuf::from) else {
            return;
        };
        self.capture()
            .save_png(&directory.join(format!("{name}.png")))
            .expect("snapshot is written");
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn capture(&self) -> Snapshot {
        let row = self.size[0] * 4;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * self.size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(self.size[1]),
                },
            },
            self.target.size(),
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("readback completes");
        let rgba = buffer
            .slice(..)
            .get_mapped_range()
            .expect("buffer is mapped")
            .to_vec();
        Snapshot {
            width: self.size[0],
            height: self.size[1],
            rgba,
        }
    }
}
