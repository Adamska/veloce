use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use super::{equalizer::EqualizerSettings, silence::SilenceSettings};

pub const MIN_SPEED: f32 = 0.25;
pub const MAX_SPEED: f32 = 4.0;
pub const MAX_VOLUME: f32 = 2.0;

pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

pub struct AudioControls {
    pub speed: AtomicF32,
    pub effective_speed: AtomicF32,
    pub volume: AtomicF32,
    pub muted: AtomicBool,
    pub paused: AtomicBool,
    pub delay_micros: AtomicI64,
    pub live_epoch: AtomicU64,
    pub callback_frames: AtomicU32,
    pub underruns: AtomicU64,
    pub equalizer: EqualizerSettings,
    pub silence: SilenceSettings,
}

impl Default for AudioControls {
    fn default() -> Self {
        Self {
            speed: AtomicF32::new(1.0),
            effective_speed: AtomicF32::new(1.0),
            volume: AtomicF32::new(1.0),
            muted: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            delay_micros: AtomicI64::new(0),
            live_epoch: AtomicU64::new(0),
            callback_frames: AtomicU32::new(512),
            underruns: AtomicU64::new(0),
            equalizer: EqualizerSettings::default(),
            silence: SilenceSettings::default(),
        }
    }
}

impl AudioControls {
    pub fn set_speed(&self, speed: f32) {
        self.speed.store(speed.clamp(MIN_SPEED, MAX_SPEED));
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.store(volume.clamp(0.0, MAX_VOLUME));
    }

    pub fn gain(&self) -> f32 {
        if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            self.volume.load()
        }
    }
}
