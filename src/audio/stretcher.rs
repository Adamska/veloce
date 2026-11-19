use signalsmith_stretch::Stretch;

use crate::media::AUDIO_CHANNELS;

pub struct TimeStretcher {
    inner: Stretch,
    input_latency: usize,
    output_latency: usize,
}

impl TimeStretcher {
    pub fn new(sample_rate: u32) -> Self {
        let inner = Stretch::preset_default(AUDIO_CHANNELS as u32, sample_rate);
        Self {
            input_latency: inner.input_latency(),
            output_latency: inner.output_latency(),
            inner,
        }
    }

    pub fn input_latency(&self) -> usize {
        self.input_latency
    }

    pub fn output_latency(&self) -> usize {
        self.output_latency
    }

    pub fn latency_in_source_frames(&self, speed: f32) -> f64 {
        self.input_latency as f64 + self.output_latency as f64 * f64::from(speed)
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn prime(&mut self, preroll: &[f32], speed: f32) {
        self.inner.seek(preroll, f64::from(speed));
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        self.inner.process(input, output);
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    const RATE: u32 = 48_000;
    const BLOCK: usize = 512;

    fn tone(frequency: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let value = (TAU * frequency * n as f32 / RATE as f32).sin() * 0.5;
                [value, value]
            })
            .collect()
    }

    fn stretch(input: &[f32], speed: f32) -> Vec<f32> {
        let mut stretcher = TimeStretcher::new(RATE);
        let mut output = Vec::new();
        let mut block = vec![0.0; BLOCK * AUDIO_CHANNELS];
        let step = (BLOCK as f32 * speed).round() as usize * AUDIO_CHANNELS;
        for chunk in input.chunks_exact(step) {
            stretcher.process(chunk, &mut block);
            output.extend_from_slice(&block);
        }
        output
    }

    fn dominant_frequency(interleaved: &[f32]) -> f32 {
        let mono: Vec<f32> = interleaved
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[0])
            .collect();
        let crossings = mono
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        crossings as f32 * RATE as f32 / mono.len() as f32
    }

    fn rms(interleaved: &[f32]) -> f32 {
        (interleaved.iter().map(|s| s * s).sum::<f32>() / interleaved.len() as f32).sqrt()
    }

    #[test]
    fn pitch_is_preserved_across_the_speed_range() {
        let input = tone(440.0, RATE as usize * 4);
        for speed in [0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0] {
            let output = stretch(&input, speed);
            let steady = &output[output.len() / 4..output.len() * 3 / 4];
            let frequency = dominant_frequency(steady);
            assert!(
                (frequency - 440.0).abs() < 6.0,
                "{frequency} Hz at {speed}x"
            );
            assert!(
                (rms(steady) - 0.3535).abs() < 0.06,
                "level drifted at {speed}x"
            );
        }
    }

    #[test]
    fn output_duration_scales_inversely_with_speed() {
        let input = tone(330.0, RATE as usize * 2);
        let output = stretch(&input, 2.0);
        let expected = input.len() / 2;
        assert!(output.len().abs_diff(expected) <= BLOCK * AUDIO_CHANNELS);
    }

    #[test]
    fn reported_latency_matches_the_measured_delay() {
        for speed in [1.0f32, 2.0] {
            let onset = RATE as usize;
            let mut input = vec![0.0; onset * AUDIO_CHANNELS];
            input.extend(tone(1_000.0, RATE as usize * 2));
            let stretcher = TimeStretcher::new(RATE);
            let output = stretch(&input, speed);
            let measured = output
                .as_chunks::<2>()
                .0
                .iter()
                .position(|frame| frame[0].abs() > 0.25)
                .expect("the tone reaches the output");
            let latency = stretcher.latency_in_source_frames(speed);
            let expected = (onset as f64 + latency) / f64::from(speed);
            let tolerance = f64::from(RATE) * 0.03;
            assert!(
                (measured as f64 - expected).abs() < tolerance,
                "measured {measured}, expected {expected} at {speed}x"
            );
        }
    }
}
