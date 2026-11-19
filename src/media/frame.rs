use std::sync::Arc;

use parking_lot::Mutex;

pub type Micros = i64;

pub const MICROS_PER_SECOND: Micros = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelLayout {
    Nv12,
    P010,
    Yuv420p,
    Yuv420p10,
}

impl PixelLayout {
    pub fn plane_count(self) -> usize {
        match self {
            Self::Nv12 | Self::P010 => 2,
            Self::Yuv420p | Self::Yuv420p10 => 3,
        }
    }

    pub fn bytes_per_component(self) -> usize {
        match self {
            Self::Nv12 | Self::Yuv420p => 1,
            Self::P010 | Self::Yuv420p10 => 2,
        }
    }

    pub fn components(self, plane: usize) -> usize {
        match (self, plane) {
            (Self::Nv12 | Self::P010, 1) => 2,
            _ => 1,
        }
    }

    pub fn plane_size(self, plane: usize, width: u32, height: u32) -> (u32, u32) {
        if plane == 0 {
            (width, height)
        } else {
            (width.div_ceil(2), height.div_ceil(2))
        }
    }

    pub fn row_bytes(self, plane: usize, width: u32) -> usize {
        let (plane_width, _) = self.plane_size(plane, width, 2);
        plane_width as usize * self.components(plane) * self.bytes_per_component()
    }

    pub fn is_high_bit_depth(self) -> bool {
        self.bytes_per_component() == 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorMatrix {
    Bt601,
    #[default]
    Bt709,
    Bt2020,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Transfer {
    #[default]
    Sdr,
    Pq,
    Hlg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ColorInfo {
    pub matrix: ColorMatrix,
    pub transfer: Transfer,
    pub full_range: bool,
}

impl ColorInfo {
    pub fn is_hdr(self) -> bool {
        self.transfer != Transfer::Sdr
    }
}

#[derive(Clone, Copy)]
pub struct PlaneRef<'a> {
    pub data: &'a [u8],
    pub stride: usize,
}

pub trait PlaneStorage: Send + Sync {
    fn plane(&self, index: usize) -> PlaneRef<'_>;
    fn byte_size(&self) -> usize;
}

pub struct VideoFrame {
    pub pts: Micros,
    pub duration: Micros,
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f32,
    pub layout: PixelLayout,
    pub color: ColorInfo,
    pub interlaced: bool,
    pub storage: Box<dyn PlaneStorage>,
}

impl VideoFrame {
    pub fn plane(&self, index: usize) -> PlaneRef<'_> {
        self.storage.plane(index)
    }

    pub fn byte_size(&self) -> usize {
        self.storage.byte_size()
    }

    pub fn end(&self) -> Micros {
        self.pts + self.duration
    }

    pub fn display_aspect(&self) -> f32 {
        self.width as f32 * self.pixel_aspect / self.height.max(1) as f32
    }
}

#[derive(Clone, Debug, Default)]
pub struct AudioChunk {
    pub pts: Micros,
    pub samples: Vec<f32>,
}

pub const AUDIO_CHANNELS: usize = 2;

impl AudioChunk {
    pub fn frames(&self) -> usize {
        self.samples.len() / AUDIO_CHANNELS
    }
}

#[derive(Default)]
pub struct BufferPool {
    free: Mutex<Vec<Vec<u8>>>,
}

const POOL_LIMIT: usize = 24;

impl BufferPool {
    pub fn take(&self, len: usize) -> Vec<u8> {
        let recycled = {
            let mut free = self.free.lock();
            free.iter()
                .position(|buffer| buffer.len() == len)
                .map(|index| free.swap_remove(index))
        };
        recycled.unwrap_or_else(|| vec![0; len])
    }

    fn give(&self, buffer: Vec<u8>) {
        let mut free = self.free.lock();
        if free.len() < POOL_LIMIT {
            free.push(buffer);
        }
    }
}

pub struct PooledPlanes {
    planes: Vec<(Vec<u8>, usize)>,
    pool: Arc<BufferPool>,
}

impl PooledPlanes {
    pub fn new(pool: &Arc<BufferPool>, layout: PixelLayout, width: u32, height: u32) -> Self {
        let planes = (0..layout.plane_count())
            .map(|plane| {
                let stride = layout.row_bytes(plane, width);
                let (_, rows) = layout.plane_size(plane, width, height);
                (pool.take(stride * rows as usize), stride)
            })
            .collect();
        Self {
            planes,
            pool: pool.clone(),
        }
    }

    pub fn planes_mut(&mut self) -> impl Iterator<Item = (&mut [u8], usize)> {
        self.planes
            .iter_mut()
            .map(|(data, stride)| (data.as_mut_slice(), *stride))
    }
}

impl PlaneStorage for PooledPlanes {
    fn plane(&self, index: usize) -> PlaneRef<'_> {
        let (data, stride) = &self.planes[index];
        PlaneRef {
            data,
            stride: *stride,
        }
    }

    fn byte_size(&self) -> usize {
        self.planes.iter().map(|(data, _)| data.len()).sum()
    }
}

impl Drop for PooledPlanes {
    fn drop(&mut self) {
        for (buffer, _) in self.planes.drain(..) {
            self.pool.give(buffer);
        }
    }
}
