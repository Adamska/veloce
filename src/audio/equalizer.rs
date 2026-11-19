use std::{
    f32::consts::PI,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use super::params::AtomicF32;
use crate::media::AUDIO_CHANNELS;

pub const BAND_COUNT: usize = 10;
pub const BAND_FREQUENCIES: [f32; BAND_COUNT] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
pub const GAIN_RANGE_DB: f32 = 12.0;
pub const PREAMP_RANGE_DB: f32 = 20.0;
const BAND_Q: f32 = 1.41;

pub const PRESETS: [(&str, [f32; BAND_COUNT]); 6] = [
    ("Flat", [0.0; BAND_COUNT]),
    (
        "Bass Boost",
        [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ),
    (
        "Dialogue",
        [-4.0, -3.0, -1.0, 1.0, 3.0, 4.0, 4.0, 2.0, 0.0, -1.0],
    ),
    (
        "Treble Boost",
        [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0],
    ),
    (
        "Loudness",
        [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.0, 4.0, 5.0],
    ),
    (
        "Night",
        [-6.0, -4.0, -2.0, 0.0, 2.0, 3.0, 3.0, 1.0, -1.0, -2.0],
    ),
];

pub struct EqualizerSettings {
    enabled: AtomicBool,
    preamp_db: AtomicF32,
    gains_db: [AtomicF32; BAND_COUNT],
    revision: AtomicU64,
}

impl Default for EqualizerSettings {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            preamp_db: AtomicF32::new(0.0),
            gains_db: std::array::from_fn(|_| AtomicF32::new(0.0)),
            revision: AtomicU64::new(1),
        }
    }
}

impl EqualizerSettings {
    fn touch(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.touch();
    }

    pub fn preamp_db(&self) -> f32 {
        self.preamp_db.load()
    }

    pub fn set_preamp_db(&self, value: f32) {
        self.preamp_db
            .store(value.clamp(-PREAMP_RANGE_DB, PREAMP_RANGE_DB));
        self.touch();
    }

    pub fn gains_db(&self) -> [f32; BAND_COUNT] {
        std::array::from_fn(|band| self.gains_db[band].load())
    }

    pub fn set_gain_db(&self, band: usize, value: f32) {
        if let Some(gain) = self.gains_db.get(band) {
            gain.store(value.clamp(-GAIN_RANGE_DB, GAIN_RANGE_DB));
            self.touch();
        }
    }

    pub fn apply_preset(&self, gains: &[f32; BAND_COUNT]) {
        for (band, gain) in gains.iter().enumerate() {
            self.gains_db[band].store(*gain);
        }
        self.touch();
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    state: [[f32; 2]; AUDIO_CHANNELS],
}

impl Biquad {
    fn peaking(frequency: f32, gain_db: f32, q: f32, sample_rate: f32) -> ([f32; 3], [f32; 2]) {
        let amplitude = 10f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * (frequency / sample_rate).min(0.49);
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha / amplitude;
        (
            [
                (1.0 + alpha * amplitude) / a0,
                -2.0 * omega.cos() / a0,
                (1.0 - alpha * amplitude) / a0,
            ],
            [-2.0 * omega.cos() / a0, (1.0 - alpha / amplitude) / a0],
        )
    }

    #[inline]
    fn tick(&mut self, channel: usize, input: f32) -> f32 {
        let state = &mut self.state[channel];
        let output = self.b[0] * input + state[0];
        state[0] = self.b[1] * input - self.a[0] * output + state[1];
        state[1] = self.b[2] * input - self.a[1] * output;
        output
    }
}

pub struct Equalizer {
    bands: [Biquad; BAND_COUNT],
    active: [bool; BAND_COUNT],
    preamp: f32,
    enabled: bool,
    revision: u64,
    sample_rate: f32,
}

impl Equalizer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            bands: [Biquad::default(); BAND_COUNT],
            active: [false; BAND_COUNT],
            preamp: 1.0,
            enabled: false,
            revision: 0,
            sample_rate: sample_rate as f32,
        }
    }

    fn refresh(&mut self, settings: &EqualizerSettings) {
        let revision = settings.revision.load(Ordering::Acquire);
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        self.enabled = settings.is_enabled();
        self.preamp = 10f32.powf(settings.preamp_db() / 20.0);
        for (band, gain_db) in settings.gains_db().into_iter().enumerate() {
            let (b, a) = Biquad::peaking(BAND_FREQUENCIES[band], gain_db, BAND_Q, self.sample_rate);
            self.bands[band].b = b;
            self.bands[band].a = a;
            self.active[band] = gain_db.abs() > 0.05;
        }
    }

    pub fn process(&mut self, settings: &EqualizerSettings, interleaved: &mut [f32]) {
        self.refresh(settings);
        if !self.enabled {
            return;
        }
        for frame in interleaved.as_chunks_mut::<AUDIO_CHANNELS>().0 {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut value = *sample * self.preamp;
                for (band, filter) in self.bands.iter_mut().enumerate() {
                    if self.active[band] {
                        value = filter.tick(channel, value);
                    }
                }
                *sample = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_gain_db(settings: &EqualizerSettings, frequency: f32) -> f32 {
        let rate = 48_000;
        let mut equalizer = Equalizer::new(rate);
        let mut samples: Vec<f32> = (0..rate as usize)
            .flat_map(|n| {
                let value = (2.0 * PI * frequency * n as f32 / rate as f32).sin() * 0.25;
                [value, value]
            })
            .collect();
        let power = |data: &[f32]| data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32;
        let before = power(&samples[rate as usize..]);
        equalizer.process(settings, &mut samples);
        10.0 * (power(&samples[rate as usize..]) / before).log10()
    }

    #[test]
    fn disabled_equalizer_is_bit_transparent() {
        let settings = EqualizerSettings::default();
        settings.set_gain_db(5, 9.0);
        let mut samples = vec![0.1, -0.2, 0.3, -0.4];
        Equalizer::new(48_000).process(&settings, &mut samples);
        assert_eq!(samples, vec![0.1, -0.2, 0.3, -0.4]);
    }

    #[test]
    fn band_gain_is_applied_at_its_center_frequency() {
        let settings = EqualizerSettings::default();
        settings.set_enabled(true);
        settings.set_gain_db(5, 6.0);
        assert!((rms_gain_db(&settings, 1_000.0) - 6.0).abs() < 0.3);
        assert!(rms_gain_db(&settings, 62.5).abs() < 0.3);
        assert!(rms_gain_db(&settings, 12_000.0).abs() < 0.3);
    }

    #[test]
    fn preamp_scales_the_whole_spectrum() {
        let settings = EqualizerSettings::default();
        settings.set_enabled(true);
        settings.set_preamp_db(-6.0);
        assert!((rms_gain_db(&settings, 440.0) + 6.0).abs() < 0.1);
    }

    #[test]
    fn presets_and_limits_are_respected() {
        let settings = EqualizerSettings::default();
        settings.set_gain_db(0, 40.0);
        assert_eq!(settings.gains_db()[0], GAIN_RANGE_DB);
        settings.apply_preset(&PRESETS[1].1);
        assert_eq!(settings.gains_db(), PRESETS[1].1);
    }
}
