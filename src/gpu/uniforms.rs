use bytemuck::{Pod, Zeroable};

use super::filters::{Placement, VideoAdjustments};
use crate::media::{ColorInfo, ColorMatrix, PixelLayout, Transfer, VideoFrame};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct VideoUniforms {
    yuv_to_rgb: [[f32; 4]; 4],
    placement: [f32; 4],
    uv_row_u: [f32; 4],
    uv_row_v: [f32; 4],
    texel: [f32; 4],
    grade: [f32; 4],
    detail: [f32; 4],
    flags: [u32; 4],
}

pub fn yuv_to_rgb(color: ColorInfo, layout: PixelLayout) -> [[f32; 4]; 4] {
    let (kr, kb) = match color.matrix {
        ColorMatrix::Bt601 => (0.299, 0.114),
        ColorMatrix::Bt709 => (0.2126, 0.0722),
        ColorMatrix::Bt2020 => (0.2627, 0.0593),
    };
    let kg = 1.0 - kr - kb;
    let (peak, code_max, black, mid): (f32, f32, f32, f32) = match layout {
        PixelLayout::Nv12 | PixelLayout::Yuv420p => (255.0, 255.0, 16.0, 128.0),
        PixelLayout::Yuv420p10 => (1023.0, 1023.0, 64.0, 512.0),
        PixelLayout::P010 => (65535.0 / 64.0, 1023.0, 64.0, 512.0),
    };
    let (luma_offset, luma_scale, chroma_scale) = if color.full_range {
        (0.0, peak / code_max, peak / code_max)
    } else {
        let step = mid / 128.0;
        (black / peak, peak / (219.0 * step), peak / (224.0 * step))
    };
    let chroma_offset = mid / peak;

    let luma = [luma_scale, luma_scale, luma_scale];
    let cb = [
        0.0,
        -2.0 * kb * (1.0 - kb) / kg * chroma_scale,
        2.0 * (1.0 - kb) * chroma_scale,
    ];
    let cr = [
        2.0 * (1.0 - kr) * chroma_scale,
        -2.0 * kr * (1.0 - kr) / kg * chroma_scale,
        0.0,
    ];
    let bias: [f32; 3] =
        std::array::from_fn(|i| -(luma[i] * luma_offset + (cb[i] + cr[i]) * chroma_offset));
    [
        [luma[0], luma[1], luma[2], 0.0],
        [cb[0], cb[1], cb[2], 0.0],
        [cr[0], cr[1], cr[2], 0.0],
        [bias[0], bias[1], bias[2], 1.0],
    ]
}

pub fn convert(matrix: &[[f32; 4]; 4], yuv: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| {
        matrix[0][i] * yuv[0] + matrix[1][i] * yuv[1] + matrix[2][i] * yuv[2] + matrix[3][i]
    })
}

impl VideoUniforms {
    pub fn new(
        frame: &VideoFrame,
        adjustments: &VideoAdjustments,
        placement: &Placement,
        blend: f32,
        linear_target: bool,
    ) -> Self {
        let sample_scale = match frame.layout {
            PixelLayout::Yuv420p10 => 65535.0 / 1023.0,
            _ => 1.0,
        };
        let planar = matches!(frame.layout, PixelLayout::Yuv420p | PixelLayout::Yuv420p10);
        let transfer = match frame.color.transfer {
            Transfer::Sdr => 0,
            Transfer::Pq => 1,
            Transfer::Hlg => 2,
        };
        let [row_u, row_v] = placement.uv_rows;
        Self {
            yuv_to_rgb: yuv_to_rgb(frame.color, frame.layout),
            placement: [
                placement.scale[0],
                placement.scale[1],
                placement.offset[0],
                placement.offset[1],
            ],
            uv_row_u: [row_u[0], row_u[1], row_u[2], 0.0],
            uv_row_v: [row_v[0], row_v[1], row_v[2], 0.0],
            texel: [
                1.0 / frame.width as f32,
                1.0 / frame.height as f32,
                frame.width as f32,
                frame.height as f32,
            ],
            grade: [
                adjustments.brightness,
                adjustments.contrast,
                adjustments.saturation,
                adjustments.gamma.max(0.05),
            ],
            detail: [
                adjustments.hue_degrees.to_radians(),
                adjustments.sharpen,
                blend.clamp(0.0, 1.0),
                sample_scale,
            ],
            flags: [
                u32::from(planar),
                transfer,
                u32::from(adjustments.deinterlace),
                u32::from(linear_target),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(matrix: ColorMatrix, full_range: bool) -> ColorInfo {
        ColorInfo {
            matrix,
            transfer: Transfer::Sdr,
            full_range,
        }
    }

    fn assert_rgb(actual: [f32; 3], expected: [f32; 3]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() < 4e-3, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn limited_range_black_and_white_map_to_the_full_swing() {
        let matrix = yuv_to_rgb(info(ColorMatrix::Bt709, false), PixelLayout::Nv12);
        assert_rgb(
            convert(&matrix, [16.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0]),
            [0.0; 3],
        );
        assert_rgb(
            convert(&matrix, [235.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0]),
            [1.0; 3],
        );
    }

    #[test]
    fn bt709_primary_red_round_trips() {
        let matrix = yuv_to_rgb(info(ColorMatrix::Bt709, false), PixelLayout::Nv12);
        let yuv = [63.0 / 255.0, 102.0 / 255.0, 240.0 / 255.0];
        assert_rgb(convert(&matrix, yuv), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn bt601_primary_blue_round_trips() {
        let matrix = yuv_to_rgb(info(ColorMatrix::Bt601, false), PixelLayout::Yuv420p);
        let yuv = [41.0 / 255.0, 240.0 / 255.0, 110.0 / 255.0];
        assert_rgb(convert(&matrix, yuv), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn full_range_keeps_luma_untouched() {
        let matrix = yuv_to_rgb(info(ColorMatrix::Bt2020, true), PixelLayout::Nv12);
        assert_rgb(
            convert(&matrix, [0.5, 128.0 / 255.0, 128.0 / 255.0]),
            [0.5; 3],
        );
    }

    #[test]
    fn ten_bit_layouts_use_their_own_quantization() {
        let limited = info(ColorMatrix::Bt2020, false);
        let p010 = |code: f32| code * 64.0 / 65535.0;
        let matrix = yuv_to_rgb(limited, PixelLayout::P010);
        assert_rgb(
            convert(&matrix, [p010(64.0), p010(512.0), p010(512.0)]),
            [0.0; 3],
        );
        assert_rgb(
            convert(&matrix, [p010(940.0), p010(512.0), p010(512.0)]),
            [1.0; 3],
        );
        let planar = yuv_to_rgb(limited, PixelLayout::Yuv420p10);
        let white = convert(&planar, [940.0 / 1023.0, 512.0 / 1023.0, 512.0 / 1023.0]);
        for channel in white {
            assert!((channel - 1.0).abs() < 1e-4, "{white:?}");
        }
    }
}
