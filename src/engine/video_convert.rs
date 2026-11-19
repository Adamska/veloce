use anyhow::{Context as _, Result};
use ffmpeg_next::{
    color, ffi,
    format::Pixel,
    frame,
    software::scaling::{self, Flags},
};

use crate::media::{
    ColorInfo, ColorMatrix, Micros, PixelLayout, PlaneRef, PlaneStorage, Transfer, VideoFrame,
};

struct DecodedPlanes {
    frame: frame::Video,
    planes: usize,
}

impl PlaneStorage for DecodedPlanes {
    fn plane(&self, index: usize) -> PlaneRef<'_> {
        PlaneRef {
            data: self.frame.data(index),
            stride: self.frame.stride(index),
        }
    }

    fn byte_size(&self) -> usize {
        (0..self.planes)
            .map(|index| self.frame.data(index).len())
            .sum()
    }
}

pub struct FrameConverter {
    norm16: bool,
    scaler: Option<(scaling::Context, (Pixel, u32, u32, Pixel))>,
}

fn bit_depth(format: Pixel) -> i32 {
    // SAFETY: the descriptor table is static; a null result is handled.
    unsafe {
        ffi::av_pix_fmt_desc_get(format.into())
            .as_ref()
            .map_or(8, |descriptor| descriptor.comp[0].depth)
    }
}

fn native_layout(format: Pixel, norm16: bool) -> Option<PixelLayout> {
    match format {
        Pixel::NV12 => Some(PixelLayout::Nv12),
        Pixel::YUV420P | Pixel::YUVJ420P => Some(PixelLayout::Yuv420p),
        Pixel::P010LE if norm16 => Some(PixelLayout::P010),
        Pixel::YUV420P10LE if norm16 => Some(PixelLayout::Yuv420p10),
        _ => None,
    }
}

fn color_info(picture: &frame::Video) -> ColorInfo {
    let matrix = match picture.color_space() {
        color::Space::BT709 => ColorMatrix::Bt709,
        color::Space::BT470BG | color::Space::SMPTE170M => ColorMatrix::Bt601,
        color::Space::BT2020NCL | color::Space::BT2020CL => ColorMatrix::Bt2020,
        _ if picture.height() >= 720 || picture.width() >= 1280 => ColorMatrix::Bt709,
        _ => ColorMatrix::Bt601,
    };
    let transfer = match picture.color_transfer_characteristic() {
        color::TransferCharacteristic::SMPTE2084 => Transfer::Pq,
        color::TransferCharacteristic::ARIB_STD_B67 => Transfer::Hlg,
        _ => Transfer::Sdr,
    };
    let full_range = picture.color_range() == color::Range::JPEG
        || matches!(
            picture.format(),
            Pixel::YUVJ420P | Pixel::YUVJ422P | Pixel::YUVJ444P
        );
    ColorInfo {
        matrix,
        transfer,
        full_range,
    }
}

impl FrameConverter {
    pub fn new(norm16: bool) -> Self {
        Self {
            norm16,
            scaler: None,
        }
    }

    fn rescale(&mut self, picture: &frame::Video) -> Result<(frame::Video, PixelLayout)> {
        let (target, layout) = if self.norm16 && bit_depth(picture.format()) > 8 {
            (Pixel::P010LE, PixelLayout::P010)
        } else {
            (Pixel::NV12, PixelLayout::Nv12)
        };
        let key = (picture.format(), picture.width(), picture.height(), target);
        if self
            .scaler
            .as_ref()
            .is_none_or(|(_, current)| *current != key)
        {
            let context =
                scaling::Context::get(key.0, key.1, key.2, target, key.1, key.2, Flags::BILINEAR)
                    .with_context(|| format!("no conversion path from {:?}", key.0))?;
            self.scaler = Some((context, key));
        }
        let mut converted = frame::Video::empty();
        if let Some((scaler, _)) = self.scaler.as_mut() {
            scaler.run(picture, &mut converted)?;
        }
        Ok((converted, layout))
    }

    pub fn convert(
        &mut self,
        picture: frame::Video,
        pts: Micros,
        duration: Micros,
    ) -> Result<VideoFrame> {
        let color = color_info(&picture);
        let interlaced = picture.is_interlaced();
        let aspect = picture.aspect_ratio();
        let pixel_aspect = if aspect.numerator() > 0 && aspect.denominator() > 0 {
            aspect.numerator() as f32 / aspect.denominator() as f32
        } else {
            1.0
        };
        let (picture, layout) = match native_layout(picture.format(), self.norm16) {
            Some(layout) => (picture, layout),
            None => self.rescale(&picture)?,
        };
        Ok(VideoFrame {
            pts,
            duration,
            width: picture.width(),
            height: picture.height(),
            pixel_aspect,
            layout,
            color,
            interlaced,
            storage: Box::new(DecodedPlanes {
                planes: layout.plane_count(),
                frame: picture,
            }),
        })
    }
}
