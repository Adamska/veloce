use std::sync::Arc;

use super::{
    filters::{Placement, VideoAdjustments},
    textures::{self, FrameTextures},
    uniforms::VideoUniforms,
};
use crate::media::VideoFrame;

const SLOT_COUNT: usize = 3;

#[derive(Clone)]
pub struct FrameSelection {
    pub primary: Arc<VideoFrame>,
    pub blend: Option<(Arc<VideoFrame>, f32)>,
}

pub struct VideoPipeline {
    pipeline: wgpu::RenderPipeline,
    plane_layout: wgpu::BindGroupLayout,
    globals: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    slots: [FrameTextures; SLOT_COUNT],
    active: Option<(usize, usize)>,
    generation: u64,
    linear_target: bool,
    pub uploads: u64,
}

impl VideoPipeline {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/video.wgsl"));
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let plane_layout = textures::plane_layout(device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&plane_layout),
                Some(&plane_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video uniforms"),
            size: size_of::<VideoUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video planes"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            pipeline,
            plane_layout,
            globals,
            uniform_buffer,
            slots: Default::default(),
            active: None,
            generation: 0,
            linear_target: target_format.is_srgb(),
            uploads: 0,
        }
    }

    fn slot_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Arc<VideoFrame>,
    ) -> usize {
        let generation = self.generation;
        let index = match self.slots.iter().position(|slot| slot.holds(frame)) {
            Some(index) => index,
            None => {
                let index = (0..SLOT_COUNT)
                    .filter(|index| self.slots[*index].last_used != generation)
                    .min_by_key(|index| self.slots[*index].last_used)
                    .unwrap_or(0);
                self.slots[index].upload(device, queue, &self.plane_layout, frame);
                self.uploads += 1;
                index
            }
        };
        self.slots[index].last_used = generation;
        index
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        selection: &FrameSelection,
        adjustments: &VideoAdjustments,
        placement: &Placement,
    ) {
        self.generation += 1;
        let primary = self.slot_for(device, queue, &selection.primary);
        let (secondary, blend) = match &selection.blend {
            Some((frame, weight)) if frame.layout == selection.primary.layout => {
                (self.slot_for(device, queue, frame), *weight)
            }
            _ => (primary, 0.0),
        };
        let uniforms = VideoUniforms::new(
            &selection.primary,
            adjustments,
            placement,
            blend,
            self.linear_target,
        );
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.active = Some((primary, secondary));
    }

    pub fn clear(&mut self) {
        self.active = None;
        for slot in &mut self.slots {
            slot.clear();
        }
    }

    pub fn displayed(&self) -> Option<&Arc<VideoFrame>> {
        self.active
            .and_then(|(primary, _)| self.slots[primary].frame())
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((primary, secondary)) = self.active else {
            return;
        };
        let (Some(current), Some(blend)) = (
            self.slots[primary].bind_group(),
            self.slots[secondary].bind_group(),
        ) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals, &[]);
        pass.set_bind_group(1, current, &[]);
        pass.set_bind_group(2, blend, &[]);
        pass.draw(0..4, 0..1);
    }
}
