use std::sync::atomic::{AtomicBool, Ordering};

use super::params::AtomicF32;

const HOLD_SECONDS: f32 = 0.35;
const ATTACK_SECONDS: f32 = 0.12;
const RELEASE_SECONDS: f32 = 0.03;

pub struct SilenceSettings {
    pub enabled: AtomicBool,
    pub threshold_db: AtomicF32,
    pub boost_speed: AtomicF32,
    pub active: AtomicBool,
}

impl Default for SilenceSettings {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            threshold_db: AtomicF32::new(-42.0),
            boost_speed: AtomicF32::new(3.0),
            active: AtomicBool::new(false),
        }
    }
}

pub fn rms_db(interleaved: &[f32]) -> f32 {
    if interleaved.is_empty() {
        return f32::NEG_INFINITY;
    }
    let power = interleaved
        .iter()
        .map(|sample| sample * sample)
        .sum::<f32>()
        / interleaved.len() as f32;
    10.0 * power.max(1e-12).log10()
}

pub struct SilenceDetector {
    sample_rate: f32,
    quiet_seconds: f32,
    blend: f32,
}

impl SilenceDetector {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate as f32,
            quiet_seconds: 0.0,
            blend: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.quiet_seconds = 0.0;
        self.blend = 0.0;
    }

    pub fn observe(&mut self, settings: &SilenceSettings, interleaved: &[f32]) {
        let seconds = interleaved.len() as f32 / 2.0 / self.sample_rate;
        let enabled = settings.enabled.load(Ordering::Relaxed);
        if enabled && rms_db(interleaved) < settings.threshold_db.load() {
            self.quiet_seconds += seconds;
        } else {
            self.quiet_seconds = 0.0;
        }
        let skipping = self.quiet_seconds >= HOLD_SECONDS;
        let step = seconds
            / if skipping {
                ATTACK_SECONDS
            } else {
                RELEASE_SECONDS
            };
        self.blend = if skipping {
            (self.blend + step).min(1.0)
        } else {
            (self.blend - step).max(0.0)
        };
        settings.active.store(self.blend > 0.0, Ordering::Relaxed);
    }

    pub fn effective_speed(&self, settings: &SilenceSettings, base: f32) -> f32 {
        let boosted = settings.boost_speed.load().max(base);
        base + (boosted - base) * self.blend
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(level: f32) -> Vec<f32> {
        vec![level; 2 * 480]
    }

    fn enabled() -> SilenceSettings {
        let settings = SilenceSettings::default();
        settings.enabled.store(true, Ordering::Relaxed);
        settings
    }

    #[test]
    fn measures_rms_in_decibels() {
        assert!((rms_db(&block(1.0)) - 0.0).abs() < 1e-3);
        assert!((rms_db(&block(0.1)) + 20.0).abs() < 1e-3);
        assert!(rms_db(&block(0.0)) < -100.0);
    }

    #[test]
    fn boosts_only_after_sustained_silence() {
        let settings = enabled();
        let mut detector = SilenceDetector::new(48_000);
        for _ in 0..20 {
            detector.observe(&settings, &block(0.0));
        }
        assert_eq!(detector.effective_speed(&settings, 1.0), 1.0);
        for _ in 0..40 {
            detector.observe(&settings, &block(0.0));
        }
        assert_eq!(detector.effective_speed(&settings, 1.0), 3.0);
        assert!(settings.active.load(Ordering::Relaxed));
    }

    #[test]
    fn speech_restores_the_base_speed_quickly() {
        let settings = enabled();
        let mut detector = SilenceDetector::new(48_000);
        for _ in 0..80 {
            detector.observe(&settings, &block(0.0));
        }
        for _ in 0..4 {
            detector.observe(&settings, &block(0.2));
        }
        assert_eq!(detector.effective_speed(&settings, 1.5), 1.5);
    }

    #[test]
    fn never_slows_playback_below_the_chosen_speed() {
        let settings = enabled();
        let mut detector = SilenceDetector::new(48_000);
        for _ in 0..80 {
            detector.observe(&settings, &block(0.0));
        }
        assert_eq!(detector.effective_speed(&settings, 4.0), 4.0);
    }

    #[test]
    fn stays_idle_when_disabled() {
        let settings = SilenceSettings::default();
        let mut detector = SilenceDetector::new(48_000);
        for _ in 0..80 {
            detector.observe(&settings, &block(0.0));
        }
        assert_eq!(detector.effective_speed(&settings, 1.0), 1.0);
    }
}
