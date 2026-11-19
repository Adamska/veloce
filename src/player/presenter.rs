use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use super::Player;
use crate::{
    engine::{PipelineEvent, monotonic_nanos},
    gpu::FrameSelection,
    media::{Micros, VideoFrame},
};

const END_TOLERANCE: Micros = 20_000;
const BLEND_SPEED: f32 = 2.0;

impl Player {
    fn handle_events(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        for event in session.drain_events() {
            match event {
                PipelineEvent::Opened(info) => {
                    if self.subtitles.adopt_embedded(&info) {
                        self.notify(format!("Subtitles: {}", self.subtitles.label()));
                    }
                    if let Some(session) = self.session.as_mut() {
                        session.info = Some(*info);
                    }
                }
                PipelineEvent::Subtitle(subtitle) => self.subtitles.ingest(subtitle),
                PipelineEvent::Failed(message) => {
                    self.report_error(format!("Playback failed: {message}"));
                    self.close();
                    return;
                }
            }
        }
    }

    fn count_dropped(&mut self, frame: &VideoFrame, epoch: u64, refresh: Duration) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if let Some((shown_epoch, shown)) = self.presented
            && shown_epoch == epoch
            && frame.pts > shown
            && !self.paused
        {
            let displayable = frame.duration as f64 / f64::from(self.effective_speed().max(0.01))
                >= refresh.as_micros() as f64 * 0.95;
            let skipped = session.shared.cache.frames_between(shown, frame.pts);
            if displayable && skipped > 0 {
                session
                    .shared
                    .telemetry
                    .dropped_frames
                    .fetch_add(skipped as u64, Ordering::Relaxed);
            }
        }
        self.presented = Some((epoch, frame.pts));
    }

    pub fn update(&mut self, refresh: Duration) -> Option<FrameSelection> {
        self.handle_events();
        let shared = self.session.as_ref()?.shared.clone();
        let position = self.clock.tick(monotonic_nanos(), self.paused);
        self.position = position;
        shared.set_playhead(position);

        if let (Some(start), Some(end)) = self.loop_points
            && position >= end
        {
            self.seek_to(start);
        } else if !self.paused
            && shared.end_of_stream()
            && shared.is_ready(self.clock.epoch())
            && position >= shared.end_time() - END_TOLERANCE
        {
            self.finish();
        }

        let epoch = self.clock.epoch();
        if shared.is_ready(epoch) {
            shared.mark_presented(epoch);
        }
        let primary = shared.cache.frame_at(self.position)?;
        if let Some((probe_epoch, started)) = self.seek_probe
            && probe_epoch == epoch
            && shared.is_ready(epoch)
            && self.position < primary.end() + primary.duration
        {
            shared
                .telemetry
                .last_present_micros
                .store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
            self.seek_probe = None;
        }
        self.count_dropped(&primary, epoch, refresh);

        let speed = self.effective_speed();
        let blend = (self.adjustments.frame_blending && !self.paused && speed >= BLEND_SPEED)
            .then(|| {
                let half_step = (refresh.as_micros() as f64 * f64::from(speed) / 2.0) as Micros;
                shared.cache.frame_at(self.position - half_step)
            })
            .flatten()
            .filter(|earlier| !Arc::ptr_eq(earlier, &primary))
            .map(|earlier| (earlier, 0.5));
        Some(FrameSelection { primary, blend })
    }
}
