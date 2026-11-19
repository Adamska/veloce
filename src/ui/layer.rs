use std::time::Duration;

use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use winit::{event::WindowEvent, window::Window};

use super::theme;

pub struct EguiLayer {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: Renderer,
}

pub struct EguiFrame {
    primitives: Vec<egui::ClippedPrimitive>,
    released_textures: Vec<egui::TextureId>,
    screen: ScreenDescriptor,
    pub repaint_after: Duration,
}

impl EguiLayer {
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            Some(winit::window::Theme::Dark),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = Renderer::new(device, format, RendererOptions::default());
        Self {
            ctx,
            state,
            renderer,
        }
    }

    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn on_window_event(
        &mut self,
        window: &Window,
        event: &WindowEvent,
    ) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

    pub fn run(
        &mut self,
        window: &Window,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        build: impl FnMut(&mut egui::Ui),
    ) -> EguiFrame {
        let input = self.state.take_egui_input(window);
        let mut output = self.ctx.run_ui(input, build);
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let released_textures = output.textures_delta.free.drain().collect();
        output.textures_delta.clear();
        self.state
            .handle_platform_output(window, output.platform_output);
        let repaint_after = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |viewport| viewport.repaint_delay);
        let size = window.inner_size();
        EguiFrame {
            primitives: self.ctx.tessellate(output.shapes, output.pixels_per_point),
            released_textures,
            screen: ScreenDescriptor {
                size_in_pixels: [size.width, size.height],
                pixels_per_point: output.pixels_per_point,
            },
            repaint_after,
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EguiFrame,
    ) {
        self.renderer
            .update_buffers(device, queue, encoder, &frame.primitives, &frame.screen);
    }

    pub fn paint(&self, pass: &mut wgpu::RenderPass<'static>, frame: &EguiFrame) {
        self.renderer.render(pass, &frame.primitives, &frame.screen);
    }

    pub fn finish(&mut self, frame: EguiFrame) {
        for id in &frame.released_textures {
            self.renderer.free_texture(id);
        }
    }
}
