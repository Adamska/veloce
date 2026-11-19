use std::{fs::File, io::BufWriter, path::Path, sync::Arc};

use anyhow::{Context as _, Result, bail};

use super::{
    filters::{Placement, VideoAdjustments},
    video_pipeline::{FrameSelection, VideoPipeline},
};
use crate::media::VideoFrame;

pub const SNAPSHOT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct Snapshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut VideoPipeline,
    frame: &Arc<VideoFrame>,
    adjustments: &VideoAdjustments,
) -> Result<Snapshot> {
    let (width, height) = (frame.width, frame.height);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SNAPSHOT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let row_bytes = width * 4;
    let padded_row =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let selection = FrameSelection {
        primary: frame.clone(),
        blend: None,
    };
    pipeline.prepare(device, queue, &selection, adjustments, &Placement::IDENTITY);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("snapshot"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pipeline.draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    pipeline.clear();

    let (sender, receiver) = crossbeam_channel::bounded(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("GPU readback did not complete")?;
    match receiver.try_recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => bail!("failed to map the snapshot buffer: {error}"),
        Err(_) => bail!("the snapshot buffer was never mapped"),
    }

    let mapped = readback
        .slice(..)
        .get_mapped_range()
        .context("the snapshot buffer is not readable")?;
    let mut rgba = Vec::with_capacity((row_bytes * height) as usize);
    for row in mapped.chunks_exact(padded_row as usize) {
        rgba.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(Snapshot {
        width,
        height,
        rgba,
    })
}

impl Snapshot {
    pub fn save_png(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let file =
            File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let rgb: Vec<u8> = self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgb)?;
        writer.finish()?;
        Ok(())
    }
}
