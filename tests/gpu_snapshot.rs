use std::sync::Arc;

use veloce::{
    gpu::{
        VideoAdjustments, VideoPipeline,
        snapshot::{self, SNAPSHOT_FORMAT},
    },
    media::{BufferPool, ColorInfo, ColorMatrix, PixelLayout, PooledPlanes, Transfer, VideoFrame},
};

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let descriptor = wgpu::DeviceDescriptor {
        required_features: adapter.features() & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM,
        ..Default::default()
    };
    pollster::block_on(adapter.request_device(&descriptor)).ok()
}

fn pq_gray(nits: f64) -> Option<[u8; 3]> {
    let (device, queue) = headless_device()?;
    if !device
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
    {
        return None;
    }
    let (m1, m2, c1, c2, c3) = (0.1593017578125, 78.84375, 0.8359375, 18.8515625, 18.6875);
    let power = (nits / 10_000.0f64).powf(m1);
    let signal = ((c1 + c2 * power) / (1.0 + c3 * power)).powf(m2);
    let luma = ((64.0 + 876.0 * signal).round() as u16) << 6;
    let chroma = 512u16 << 6;

    let pool = Arc::new(BufferPool::default());
    let (width, height) = (64, 36);
    let mut planes = PooledPlanes::new(&pool, PixelLayout::P010, width, height);
    for (plane, (data, _)) in planes.planes_mut().enumerate() {
        let value = if plane == 0 { luma } else { chroma };
        for sample in data.as_chunks_mut::<2>().0 {
            sample.copy_from_slice(&value.to_le_bytes());
        }
    }
    let frame = Arc::new(VideoFrame {
        pts: 0,
        duration: 33_333,
        width,
        height,
        pixel_aspect: 1.0,
        layout: PixelLayout::P010,
        color: ColorInfo {
            matrix: ColorMatrix::Bt2020,
            transfer: Transfer::Pq,
            full_range: false,
        },
        interlaced: false,
        storage: Box::new(planes),
    });
    let mut pipeline = VideoPipeline::new(&device, SNAPSHOT_FORMAT);
    let image = snapshot::capture(
        &device,
        &queue,
        &mut pipeline,
        &frame,
        &VideoAdjustments::default(),
    )
    .expect("snapshot capture failed");
    let offset = ((18 * 64 + 32) * 4) as usize;
    Some([
        image.rgba[offset],
        image.rgba[offset + 1],
        image.rgba[offset + 2],
    ])
}

#[test]
fn hdr10_is_tone_mapped_into_the_sdr_range() {
    let (Some(black), Some(dim), Some(white), Some(highlight)) = (
        pq_gray(0.0),
        pq_gray(20.0),
        pq_gray(203.0),
        pq_gray(4_000.0),
    ) else {
        eprintln!("16-bit textures unavailable, skipping");
        return;
    };
    for pixel in [black, dim, white, highlight] {
        assert!(
            pixel[0].abs_diff(pixel[1]) <= 3 && pixel[1].abs_diff(pixel[2]) <= 3,
            "{pixel:?}"
        );
    }
    assert!(black[1] <= 3, "black {black:?}");
    assert!((60..=150).contains(&dim[1]), "20 nits {dim:?}");
    assert!((200..=245).contains(&white[1]), "reference white {white:?}");
    assert!(
        highlight[1] > white[1] && highlight[1] >= 250,
        "highlight {highlight:?}"
    );
}

fn flat_frame(layout: PixelLayout, values: [u8; 3]) -> Arc<VideoFrame> {
    let pool = Arc::new(BufferPool::default());
    let (width, height) = (64, 36);
    let mut planes = PooledPlanes::new(&pool, layout, width, height);
    for (plane, (data, _)) in planes.planes_mut().enumerate() {
        match (layout, plane) {
            (_, 0) => data.fill(values[0]),
            (PixelLayout::Nv12, _) => {
                for pair in data.as_chunks_mut::<2>().0 {
                    pair.copy_from_slice(&values[1..]);
                }
            }
            (_, index) => data.fill(values[index]),
        }
    }
    Arc::new(VideoFrame {
        pts: 0,
        duration: 16_666,
        width,
        height,
        pixel_aspect: 1.0,
        layout,
        color: ColorInfo {
            matrix: ColorMatrix::Bt709,
            transfer: Transfer::Sdr,
            full_range: false,
        },
        interlaced: false,
        storage: Box::new(planes),
    })
}

fn center_pixel(
    layout: PixelLayout,
    values: [u8; 3],
    adjustments: &VideoAdjustments,
) -> Option<[u8; 3]> {
    let (device, queue) = headless_device()?;
    let mut pipeline = VideoPipeline::new(&device, SNAPSHOT_FORMAT);
    let frame = flat_frame(layout, values);
    let image = snapshot::capture(&device, &queue, &mut pipeline, &frame, adjustments)
        .expect("snapshot capture failed");
    assert_eq!((image.width, image.height), (64, 36));
    let offset = ((18 * 64 + 32) * 4) as usize;
    Some([
        image.rgba[offset],
        image.rgba[offset + 1],
        image.rgba[offset + 2],
    ])
}

fn assert_near(actual: [u8; 3], expected: [u8; 3]) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            a.abs_diff(e) <= 3,
            "{actual:?} is not close to {expected:?}"
        );
    }
}

#[test]
fn nv12_red_converts_to_rgb_red() {
    let Some(pixel) = center_pixel(
        PixelLayout::Nv12,
        [63, 102, 240],
        &VideoAdjustments::default(),
    ) else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    assert_near(pixel, [255, 0, 0]);
}

#[test]
fn planar_mid_gray_stays_neutral() {
    let Some(pixel) = center_pixel(
        PixelLayout::Yuv420p,
        [126, 128, 128],
        &VideoAdjustments::default(),
    ) else {
        return;
    };
    assert_near(pixel, [128, 128, 128]);
}

#[test]
fn zero_saturation_removes_chroma() {
    let adjustments = VideoAdjustments {
        saturation: 0.0,
        ..VideoAdjustments::default()
    };
    let Some(pixel) = center_pixel(PixelLayout::Nv12, [63, 102, 240], &adjustments) else {
        return;
    };
    assert!(pixel[0].abs_diff(pixel[1]) <= 2 && pixel[1].abs_diff(pixel[2]) <= 2);
}

#[test]
fn brightness_lifts_every_channel() {
    let adjustments = VideoAdjustments {
        brightness: 0.2,
        ..VideoAdjustments::default()
    };
    let Some(pixel) = center_pixel(PixelLayout::Yuv420p, [126, 128, 128], &adjustments) else {
        return;
    };
    assert_near(pixel, [179, 179, 179]);
}
