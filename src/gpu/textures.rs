use std::sync::Arc;

use crate::media::{PixelLayout, VideoFrame};

pub const PLANE_SLOTS: usize = 3;

#[derive(Default)]
pub struct FrameTextures {
    frame: Option<Arc<VideoFrame>>,
    shape: Option<(PixelLayout, u32, u32)>,
    planes: Vec<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,
    pub last_used: u64,
}

fn plane_format(layout: PixelLayout, plane: usize) -> wgpu::TextureFormat {
    match (layout.is_high_bit_depth(), layout.components(plane)) {
        (false, 1) => wgpu::TextureFormat::R8Unorm,
        (false, _) => wgpu::TextureFormat::Rg8Unorm,
        (true, 1) => wgpu::TextureFormat::R16Unorm,
        (true, _) => wgpu::TextureFormat::Rg16Unorm,
    }
}

pub fn plane_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entries: Vec<_> = (0..PLANE_SLOTS as u32)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        })
        .collect();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("video planes"),
        entries: &entries,
    })
}

impl FrameTextures {
    pub fn holds(&self, frame: &Arc<VideoFrame>) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, frame))
    }

    pub fn frame(&self) -> Option<&Arc<VideoFrame>> {
        self.frame.as_ref()
    }

    pub fn bind_group(&self) -> Option<&wgpu::BindGroup> {
        self.bind_group.as_ref()
    }

    pub fn clear(&mut self) {
        self.frame = None;
    }

    fn allocate(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        frame: &VideoFrame,
    ) {
        self.planes = (0..frame.layout.plane_count())
            .map(|plane| {
                let (width, height) = frame.layout.plane_size(plane, frame.width, frame.height);
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("video plane"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: plane_format(frame.layout, plane),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .collect();
        let views: Vec<_> = self
            .planes
            .iter()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()))
            .collect();
        let entries: Vec<_> = (0..PLANE_SLOTS)
            .map(|slot| wgpu::BindGroupEntry {
                binding: slot as u32,
                resource: wgpu::BindingResource::TextureView(&views[slot.min(views.len() - 1)]),
            })
            .collect();
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video planes"),
            layout,
            entries: &entries,
        }));
        self.shape = Some((frame.layout, frame.width, frame.height));
    }

    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        frame: &Arc<VideoFrame>,
    ) {
        if self.holds(frame) {
            return;
        }
        if self.shape != Some((frame.layout, frame.width, frame.height)) {
            self.allocate(device, layout, frame);
        }
        for (plane, texture) in self.planes.iter().enumerate() {
            let source = frame.plane(plane);
            let (width, height) = frame.layout.plane_size(plane, frame.width, frame.height);
            let row_bytes = frame.layout.row_bytes(plane, frame.width);
            let required = source.stride * (height as usize - 1) + row_bytes;
            if source.stride < row_bytes || source.data.len() < required {
                log::warn!("skipping malformed video plane {plane}");
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &source.data[..required],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(source.stride as u32),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.frame = Some(frame.clone());
    }
}
