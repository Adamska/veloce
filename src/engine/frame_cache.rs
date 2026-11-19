use std::{collections::VecDeque, sync::Arc};

use parking_lot::RwLock;

use crate::media::{MICROS_PER_SECOND, Micros, VideoFrame};

const BEHIND_SHARE: usize = 2;
const AHEAD_SHARE: usize = 5;
const DEFAULT_BUDGET_MB: usize = 3072;
const GUARANTEED_AHEAD: Micros = 1_500_000;

#[derive(Clone, Copy, Debug)]
pub struct CacheLimits {
    pub behind: Micros,
    pub ahead: Micros,
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

    fn frame_bytes(&self) -> usize {
        (self.bytes / self.frames.len().max(1)).max(1)
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
        if last.end() < playhead + GUARANTEED_AHEAD {
            return true;
        }
        let ahead = inner.frames.len() - inner.index_at(playhead).map_or(0, |index| index + 1);
        let capacity = self.limits.budget_bytes / inner.frame_bytes();
        let ahead_capacity = (capacity * AHEAD_SHARE / (AHEAD_SHARE + BEHIND_SHARE)).max(2);
        ahead < ahead_capacity
    }

    pub fn trim(&self, playhead: Micros) {
        let mut evicted = Vec::new();
        {
            let mut inner = self.inner.write();
            let capacity = self.limits.budget_bytes / inner.frame_bytes();
            let behind_capacity = (capacity * BEHIND_SHARE / (AHEAD_SHARE + BEHIND_SHARE)).max(1);
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
        let evicted = {
            let mut inner = self.inner.write();
            inner.bytes = 0;
            std::mem::take(&mut inner.frames)
        };
        drop(evicted);
    }
}
