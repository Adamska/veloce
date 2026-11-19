use std::{
    sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};

use super::{
    audio_timeline::AudioTimeline,
    clock::next_epoch,
    frame_cache::{CacheLimits, FrameCache},
    telemetry::Telemetry,
};
use crate::media::Micros;

const NO_TRACK: i64 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeekRequest {
    pub target: Micros,
    pub epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekOutcome {
    Hit,
    Miss,
}

pub struct SessionShared {
    pub cache: FrameCache,
    pub audio: AudioTimeline,
    pub telemetry: Telemetry,
    pending: Mutex<Option<SeekRequest>>,
    wake: Condvar,
    epoch: AtomicU64,
    ready_epoch: AtomicU64,
    presented_epoch: AtomicU64,
    target: AtomicI64,
    playhead: AtomicI64,
    end_time: AtomicI64,
    audio_track: AtomicI64,
    has_video: AtomicBool,
    end_of_stream: AtomicBool,
    stopped: AtomicBool,
}

impl SessionShared {
    pub fn new(limits: CacheLimits, sample_rate: u32, start: Micros) -> Self {
        let epoch = next_epoch();
        Self {
            cache: FrameCache::new(limits),
            audio: AudioTimeline::new(sample_rate),
            telemetry: Telemetry::default(),
            pending: Mutex::new(Some(SeekRequest {
                target: start,
                epoch,
            })),
            wake: Condvar::new(),
            epoch: AtomicU64::new(epoch),
            ready_epoch: AtomicU64::new(0),
            presented_epoch: AtomicU64::new(0),
            target: AtomicI64::new(start),
            playhead: AtomicI64::new(start),
            end_time: AtomicI64::new(0),
            audio_track: AtomicI64::new(0),
            has_video: AtomicBool::new(true),
            end_of_stream: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    pub fn target(&self) -> Micros {
        self.target.load(Ordering::Acquire)
    }

    pub fn seek_state(&self) -> SeekRequest {
        let _guard = self.pending.lock();
        SeekRequest {
            target: self.target(),
            epoch: self.epoch(),
        }
    }

    pub fn is_ready(&self, epoch: u64) -> bool {
        self.ready_epoch.load(Ordering::Acquire) == epoch
    }

    pub fn mark_ready(&self, epoch: u64) {
        self.ready_epoch.store(epoch, Ordering::Release);
    }

    pub fn mark_presented(&self, epoch: u64) {
        self.presented_epoch.store(epoch, Ordering::Release);
    }

    pub fn can_start_audio(&self, epoch: u64) -> bool {
        self.is_ready(epoch)
            && (!self.has_video() || self.presented_epoch.load(Ordering::Acquire) == epoch)
    }

    pub fn has_video(&self) -> bool {
        self.has_video.load(Ordering::Relaxed)
    }

    pub fn set_has_video(&self, value: bool) {
        self.has_video.store(value, Ordering::Relaxed);
    }

    pub fn playhead(&self) -> Micros {
        self.playhead.load(Ordering::Relaxed)
    }

    pub fn set_playhead(&self, position: Micros) {
        self.playhead.store(position, Ordering::Relaxed);
    }

    pub fn audio_track(&self) -> Option<usize> {
        usize::try_from(self.audio_track.load(Ordering::Relaxed)).ok()
    }

    pub fn set_audio_track(&self, track: Option<usize>) {
        let value = track.map_or(NO_TRACK, |index| index as i64);
        self.audio_track.store(value, Ordering::Relaxed);
    }

    pub fn end_of_stream(&self) -> bool {
        self.end_of_stream.load(Ordering::Acquire)
    }

    pub fn end_time(&self) -> Micros {
        self.end_time.load(Ordering::Acquire)
    }

    pub fn set_end_of_stream(&self, end_time: Micros) {
        self.end_time.store(end_time, Ordering::Release);
        self.end_of_stream.store(true, Ordering::Release);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    fn window_covers(&self, target: Micros) -> bool {
        if self.has_video() {
            self.cache.covers(target)
        } else {
            self.audio.covers(target)
        }
    }

    fn begin(&self, target: Micros, force_reload: bool) -> (SeekOutcome, u64) {
        let started = Instant::now();
        let mut pending = self.pending.lock();
        let epoch = next_epoch();
        self.target.store(target, Ordering::Release);
        self.playhead.store(target, Ordering::Relaxed);
        self.epoch.store(epoch, Ordering::Release);
        let outcome = if !force_reload && self.window_covers(target) {
            *pending = None;
            self.mark_ready(epoch);
            Telemetry::bump(&self.telemetry.seeks_hit);
            SeekOutcome::Hit
        } else {
            if pending.is_some() {
                Telemetry::bump(&self.telemetry.seeks_coalesced);
            }
            *pending = Some(SeekRequest { target, epoch });
            Telemetry::bump(&self.telemetry.seeks_missed);
            SeekOutcome::Miss
        };
        drop(pending);
        self.telemetry
            .last_resolve_nanos
            .store(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.wake.notify_all();
        (outcome, epoch)
    }

    pub fn seek(&self, target: Micros) -> (SeekOutcome, u64) {
        self.begin(target, false)
    }

    pub fn reload(&self, target: Micros) -> u64 {
        self.begin(target, true).1
    }

    pub fn has_pending_seek(&self) -> bool {
        self.pending.lock().is_some()
    }

    pub fn take_seek(&self) -> Option<SeekRequest> {
        let mut pending = self.pending.lock();
        let request = pending.take()?;
        self.cache.flush();
        self.audio.flush();
        self.end_of_stream.store(false, Ordering::Release);
        Telemetry::bump(&self.telemetry.decode_flushes);
        Some(request)
    }

    pub fn park(&self, timeout: Duration) {
        let mut pending = self.pending.lock();
        if pending.is_none() && !self.is_stopped() {
            self.wake.wait_for(&mut pending, timeout);
        }
    }
}
