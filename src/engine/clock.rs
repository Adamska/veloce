use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering},
    },
    time::Instant,
};

use crate::media::Micros;

const SNAP_THRESHOLD: f64 = 40_000.0;
const CORRECTION_GAIN: f64 = 0.08;

static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

pub fn next_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}

pub fn monotonic_nanos() -> u64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Anchor {
    pub epoch: u64,
    pub media: Micros,
    pub at_nanos: u64,
    pub rate: f32,
    pub limit: Micros,
}

#[derive(Default)]
pub struct ClockAnchor {
    sequence: AtomicU64,
    epoch: AtomicU64,
    media: AtomicI64,
    at_nanos: AtomicU64,
    rate: AtomicU32,
    limit: AtomicI64,
}

impl ClockAnchor {
    pub fn publish(&self, anchor: Anchor) {
        self.sequence.fetch_add(1, Ordering::SeqCst);
        self.epoch.store(anchor.epoch, Ordering::SeqCst);
        self.media.store(anchor.media, Ordering::SeqCst);
        self.at_nanos.store(anchor.at_nanos, Ordering::SeqCst);
        self.rate.store(anchor.rate.to_bits(), Ordering::SeqCst);
        self.limit.store(anchor.limit, Ordering::SeqCst);
        self.sequence.fetch_add(1, Ordering::SeqCst);
    }

    pub fn read(&self) -> Anchor {
        loop {
            let before = self.sequence.load(Ordering::SeqCst);
            if before & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let anchor = Anchor {
                epoch: self.epoch.load(Ordering::SeqCst),
                media: self.media.load(Ordering::SeqCst),
                at_nanos: self.at_nanos.load(Ordering::SeqCst),
                rate: f32::from_bits(self.rate.load(Ordering::SeqCst)),
                limit: self.limit.load(Ordering::SeqCst),
            };
            if self.sequence.load(Ordering::SeqCst) == before {
                return anchor;
            }
        }
    }
}

pub struct PlaybackClock {
    anchor: Arc<ClockAnchor>,
    epoch: u64,
    position: Micros,
    estimate: Option<(f64, u64)>,
    correction: Micros,
    running: bool,
}

impl PlaybackClock {
    pub fn new(anchor: Arc<ClockAnchor>) -> Self {
        Self {
            anchor,
            epoch: 0,
            position: 0,
            estimate: None,
            correction: 0,
            running: false,
        }
    }

    pub fn hold(&mut self, position: Micros, epoch: u64) {
        self.position = position;
        self.epoch = epoch;
        self.estimate = None;
        self.running = false;
    }

    pub fn position(&self) -> Micros {
        self.position
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn last_correction(&self) -> Micros {
        self.correction
    }

    pub fn tick(&mut self, now_nanos: u64, paused: bool) -> Micros {
        let anchor = self.anchor.read();
        if paused || anchor.epoch != self.epoch {
            self.estimate = None;
            self.running = false;
            return self.position;
        }
        let rate = f64::from(anchor.rate);
        let since_anchor = (now_nanos as i64 - anchor.at_nanos as i64) as f64 / 1_000.0;
        let reference = (anchor.media as f64 + since_anchor * rate).min(anchor.limit as f64);
        let value = match self.estimate {
            Some((estimate, at)) => {
                let predicted = estimate + now_nanos.saturating_sub(at) as f64 / 1_000.0 * rate;
                let error = reference - predicted;
                if error.abs() > SNAP_THRESHOLD {
                    reference
                } else {
                    self.correction = (error * CORRECTION_GAIN) as Micros;
                    predicted + error * CORRECTION_GAIN
                }
            }
            None => reference,
        };
        self.estimate = Some((value, now_nanos));
        self.running = anchor.rate > 0.0;
        self.position = self.position.max(value.round() as Micros);
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn clock() -> (Arc<ClockAnchor>, PlaybackClock) {
        let anchor = Arc::new(ClockAnchor::default());
        (anchor.clone(), PlaybackClock::new(anchor))
    }

    fn anchor(epoch: u64, media: Micros, at_ms: u64, rate: f32) -> Anchor {
        Anchor {
            epoch,
            media,
            at_nanos: at_ms * MS,
            rate,
            limit: media + 50_000,
        }
    }

    #[test]
    fn anchor_round_trips_through_the_seqlock() {
        let shared = ClockAnchor::default();
        let value = anchor(7, 1_234_567, 99, 1.5);
        shared.publish(value);
        assert_eq!(shared.read(), value);
    }

    #[test]
    fn holds_the_seek_target_until_audio_of_that_epoch_plays() {
        let (shared, mut clock) = clock();
        shared.publish(anchor(1, 9_000_000, 0, 1.0));
        clock.hold(2_000_000, 2);
        assert_eq!(clock.tick(5 * MS, false), 2_000_000);
        assert!(!clock.is_running());
        shared.publish(anchor(2, 2_000_000, 10, 1.0));
        assert_eq!(clock.tick(20 * MS, false), 2_010_000);
        assert!(clock.is_running());
    }

    #[test]
    fn extrapolates_at_the_playback_rate() {
        let (shared, mut clock) = clock();
        clock.hold(0, 3);
        shared.publish(anchor(3, 0, 100, 2.0));
        assert_eq!(clock.tick(110 * MS, false), 20_000);
    }

    #[test]
    fn never_runs_past_the_submitted_audio() {
        let (shared, mut clock) = clock();
        clock.hold(0, 4);
        shared.publish(anchor(4, 0, 0, 1.0));
        assert_eq!(clock.tick(500 * MS, false), 50_000);
    }

    #[test]
    fn stays_frozen_while_paused() {
        let (shared, mut clock) = clock();
        clock.hold(750_000, 5);
        shared.publish(anchor(5, 750_000, 0, 1.0));
        assert_eq!(clock.tick(30 * MS, true), 750_000);
    }

    #[test]
    fn absorbs_callback_jitter_without_stepping_backwards() {
        let (shared, mut clock) = clock();
        clock.hold(0, 6);
        let mut previous = 0;
        for step in 0..200u64 {
            let jitter = if step % 2 == 0 { 3 } else { 0 };
            shared.publish(Anchor {
                epoch: 6,
                media: (step * 10_000) as Micros,
                at_nanos: (step * 10 + jitter) * MS,
                rate: 1.0,
                limit: i64::MAX,
            });
            let now = clock.tick((step * 10 + 5) * MS, false);
            assert!(now >= previous);
            previous = now;
        }
        let ideal = 199 * 10_000 + 5_000 - 1_500;
        assert!(
            (previous - ideal).abs() < 2_000,
            "drift {}",
            previous - ideal
        );
    }
}
