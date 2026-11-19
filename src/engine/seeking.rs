use std::{sync::atomic::Ordering, time::Instant};

use super::{
    clock::next_epoch,
    session::{SeekOutcome, SeekRequest, SessionShared},
    telemetry::Telemetry,
};
use crate::media::{AudioChunk, Micros, VideoFrame};

impl SessionShared {
    fn begin(&self, target: Micros, stride: Option<Micros>, reload: bool) -> (SeekOutcome, u64) {
        let started = Instant::now();
        let mut pending = self.pending.lock();
        let epoch = next_epoch();
        let video = self.has_video();
        self.target.store(target, Ordering::Release);
        self.playhead.store(target, Ordering::Relaxed);
        self.epoch.store(epoch, Ordering::Release);

        let mut resumed = false;
        let mut hit = !reload && self.window_covers(target);
        if !hit
            && !reload
            && let Some(decoded) = self.lanes.promote(&self.cache, &self.audio, target, video)
        {
            self.end_of_stream.store(false, Ordering::Release);
            hit = decoded && self.window_covers(target);
            resumed = true;
            if hit {
                Telemetry::bump(&self.telemetry.seeks_prepared);
            }
        }
        if reload {
            self.lanes.invalidate();
        }
        self.lanes.plan(target, stride, video);
        self.lanes.touch();

        let outcome = if hit {
            *pending = None;
            self.mark_ready(epoch);
            Telemetry::bump(&self.telemetry.seeks_hit);
            SeekOutcome::Hit
        } else {
            if pending.is_some() {
                Telemetry::bump(&self.telemetry.seeks_coalesced);
            }
            *pending = Some(SeekRequest {
                target,
                epoch,
                resumed,
            });
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
        self.begin(target, None, false)
    }

    pub fn hop(&self, target: Micros, stride: Micros) -> (SeekOutcome, u64) {
        self.begin(target, Some(stride), false)
    }

    pub fn reload(&self, target: Micros) -> u64 {
        self.begin(target, None, true).1
    }

    pub fn has_pending_seek(&self) -> bool {
        self.pending.lock().is_some()
    }

    pub fn take_seek(&self, lane: usize) -> Option<SeekRequest> {
        let mut pending = self.pending.lock();
        if self.lanes.active() != lane {
            return None;
        }
        let request = pending.take()?;
        if !request.resumed {
            self.cache.flush();
            self.audio.flush();
            Telemetry::bump(&self.telemetry.decode_flushes);
        }
        self.end_of_stream.store(false, Ordering::Release);
        Some(request)
    }

    pub fn begin_retarget(&self, lane: usize) -> bool {
        let _guard = self.pending.lock();
        let spare = self.lanes.active() != lane;
        if spare {
            self.lanes.pad(lane).clear();
        }
        spare
    }

    pub fn deliver_video(&self, lane: usize, frame: VideoFrame) -> Option<bool> {
        let _guard = self.pending.lock();
        if self.lanes.active() == lane {
            Some(self.cache.push(frame))
        } else {
            self.lanes.pad(lane).frames.push(frame);
            None
        }
    }

    pub fn deliver_audio(&self, lane: usize, chunk: AudioChunk) -> bool {
        let _guard = self.pending.lock();
        let active = self.lanes.active() == lane;
        if active {
            self.audio.push(chunk);
        } else {
            self.lanes.pad(lane).audio.push(chunk);
        }
        active
    }
}
