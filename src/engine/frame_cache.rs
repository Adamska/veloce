use std::{collections::VecDeque, sync::Arc};

use parking_lot::RwLock;

use crate::media::{MICROS_PER_SECOND, Micros, VideoFrame};

const DEFAULT_BUDGET_MB: usize = 3072;
const HOP_RESERVE: Micros = 6 * MICROS_PER_SECOND;

#[derive(Clone, Copy, Debug)]
pub struct CacheLimits {
    pub behind: Micros,
    pub ahead: Micros,
    pub guaranteed_ahead: Micros,
    pub budget_bytes: usize,
}

impl Default for CacheLimits {
    fn default() -> Self {
        let budget_mb = std::env::var("VELOCE_CACHE_MB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|megabytes| *megabytes >= 64)
            .unwrap_or(DEFAULT_BUDGET_MB);
        Self {
            behind: 10 * MICROS_PER_SECOND,
            ahead: 25 * MICROS_PER_SECOND,
            guaranteed_ahead: 1_500_000,
            budget_bytes: budget_mb * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheSpan {
    pub start: Micros,
    pub end: Micros,
    pub frames: usize,
    pub bytes: usize,
}

#[derive(Default)]
struct Inner {
    frames: VecDeque<Arc<VideoFrame>>,
    bytes: usize,
}

impl Inner {
    fn index_at(&self, time: Micros) -> Option<usize> {
        self.frames
            .partition_point(|frame| frame.pts <= time)
            .checked_sub(1)
    }

    fn capacities(&self, budget_bytes: usize) -> (usize, usize) {
        let count = self.frames.len().max(1);
        let capacity = budget_bytes / (self.bytes / count).max(1);
        let duration = match (self.frames.front(), self.frames.back()) {
            (Some(first), Some(last)) => (last.end() - first.pts).max(1),
            _ => 1,
        };
        let reserve = (count as i128 * i128::from(HOP_RESERVE) / i128::from(duration)) as usize;
        let behind = if capacity / 2 <= reserve {
            capacity / 2
        } else {
            reserve.max(capacity * 2 / 7)
        };
        (behind.max(1), capacity.saturating_sub(behind).max(2))
    }
}

pub struct FrameCache {
    inner: RwLock<Inner>,
    limits: CacheLimits,
}

impl FrameCache {
    pub fn new(limits: CacheLimits) -> Self {
        Self {
            inner: RwLock::default(),
            limits,
        }
    }

    pub fn limits(&self) -> CacheLimits {
        self.limits
    }

    pub fn push(&self, frame: VideoFrame) -> bool {
        let mut inner = self.inner.write();
        if inner
            .frames
            .back()
            .is_some_and(|last| frame.pts <= last.pts)
        {
            return false;
        }
        inner.bytes += frame.byte_size();
        inner.frames.push_back(Arc::new(frame));
        true
    }

    pub fn frame_at(&self, time: Micros) -> Option<Arc<VideoFrame>> {
        let inner = self.inner.read();
        inner
            .index_at(time)
            .map(|index| inner.frames[index].clone())
    }

    pub fn covers(&self, time: Micros) -> bool {
        let inner = self.inner.read();
        match (inner.frames.front(), inner.frames.back()) {
            (Some(first), Some(last)) => first.pts <= time && time < last.end(),
            _ => false,
        }
    }

    pub fn neighbor(&self, pts: Micros, forward: bool) -> Option<Arc<VideoFrame>> {
        let inner = self.inner.read();
        let index = if forward {
            inner.frames.partition_point(|frame| frame.pts <= pts)
        } else {
            inner
                .frames
                .partition_point(|frame| frame.pts < pts)
                .checked_sub(1)?
        };
        inner.frames.get(index).cloned()
    }

    pub fn frames_between(&self, earlier: Micros, later: Micros) -> usize {
        let inner = self.inner.read();
        let first = inner.frames.partition_point(|frame| frame.pts <= earlier);
        let last = inner.frames.partition_point(|frame| frame.pts < later);
        last.saturating_sub(first)
    }

    pub fn is_contiguous(&self, tolerance: Micros) -> bool {
        let inner = self.inner.read();
        inner
            .frames
            .iter()
            .zip(inner.frames.iter().skip(1))
            .all(|(earlier, later)| (later.pts - earlier.end()).abs() <= tolerance)
    }

    pub fn span(&self) -> Option<CacheSpan> {
        let inner = self.inner.read();
        Some(CacheSpan {
            start: inner.frames.front()?.pts,
            end: inner.frames.back()?.end(),
            frames: inner.frames.len(),
            bytes: inner.bytes,
        })
    }

    pub fn wants_more(&self, playhead: Micros) -> bool {
        let inner = self.inner.read();
        let Some(last) = inner.frames.back() else {
            return true;
        };
        if last.end() >= playhead + self.limits.ahead {
            return false;
        }
        if last.end() < playhead + self.limits.guaranteed_ahead {
            return true;
        }
        let ahead = inner.frames.len() - inner.index_at(playhead).map_or(0, |index| index + 1);
        ahead < inner.capacities(self.limits.budget_bytes).1
    }

    pub fn trim(&self, playhead: Micros) {
        let mut evicted = Vec::new();
        {
            let mut inner = self.inner.write();
            let (behind_capacity, _) = inner.capacities(self.limits.budget_bytes);
            loop {
                let behind = inner.index_at(playhead).unwrap_or(0);
                let Some(first) = inner.frames.front() else {
                    break;
                };
                let expired = first.end() < playhead - self.limits.behind;
                if behind == 0 || !(expired || behind > behind_capacity) {
                    break;
                }
                if let Some(frame) = inner.frames.pop_front() {
                    inner.bytes -= frame.byte_size();
                    evicted.push(frame);
                }
            }
        }
        drop(evicted);
    }

    pub fn flush(&self) {
        let evicted = std::mem::take(&mut *self.inner.write());
        drop(evicted);
    }

    pub fn exchange(&self, other: &Self) {
        if !std::ptr::eq(self, other) {
            std::mem::swap(&mut *self.inner.write(), &mut *other.inner.write());
        }
    }
}
