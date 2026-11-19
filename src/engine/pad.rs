use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use super::{
    audio_timeline::AudioTimeline,
    frame_cache::{CacheLimits, FrameCache},
    lanes::{AIM_SLACK, PAD_AHEAD, PAD_BEHIND, ROLL_LIMIT, UNSET},
};
use crate::media::Micros;

pub struct Pad {
    pub frames: FrameCache,
    pub audio: AudioTimeline,
    pub(super) offset: AtomicI64,
    aim: AtomicI64,
    pub(super) stale: AtomicBool,
}

impl Pad {
    pub(super) fn new(limits: CacheLimits, sample_rate: u32) -> Self {
        Self {
            frames: FrameCache::new(limits),
            audio: AudioTimeline::new(sample_rate),
            offset: AtomicI64::new(UNSET),
            aim: AtomicI64::new(UNSET),
            stale: AtomicBool::new(false),
        }
    }

    pub fn covers(&self, time: Micros, video: bool) -> bool {
        if video {
            self.frames.covers(time)
        } else {
            self.audio.covers(time)
        }
    }

    pub fn end(&self, video: bool) -> Option<Micros> {
        if video {
            self.frames.span().map(|span| span.end)
        } else {
            self.audio.end_time()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.frames.span().is_none() && self.audio.bounds().is_none()
    }

    pub fn aim(&self) -> Option<Micros> {
        Some(self.aim.load(Ordering::Acquire)).filter(|aim| *aim != UNSET)
    }

    pub fn set_aim(&self, aim: Option<Micros>) {
        self.aim.store(aim.unwrap_or(UNSET), Ordering::Release);
    }

    pub(super) fn aims_at(&self, time: Micros) -> bool {
        self.aim()
            .is_some_and(|aim| aim <= time + AIM_SLACK && time <= aim + ROLL_LIMIT)
    }

    pub fn serves(&self, time: Micros, video: bool) -> bool {
        self.covers(time, video)
            || self.aims_at(time)
            || self
                .end(video)
                .is_some_and(|end| (end..=end + ROLL_LIMIT).contains(&time))
    }

    pub fn wants_more(&self, anchor: Micros, video: bool) -> bool {
        if video {
            self.frames.wants_more(anchor)
        } else {
            self.audio
                .end_time()
                .is_none_or(|end| end < anchor + PAD_AHEAD)
        }
    }

    pub fn trim(&self, anchor: Micros) {
        self.frames.trim(anchor);
        self.audio.trim(anchor, PAD_BEHIND);
    }

    pub fn clear(&self) {
        self.frames.flush();
        self.audio.flush();
    }

    pub fn take_stale(&self) -> bool {
        self.stale.swap(false, Ordering::AcqRel)
    }
}
