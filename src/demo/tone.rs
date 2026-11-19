use std::f64::consts::TAU;

const SCALE: [i32; 8] = [0, 3, 5, 7, 10, 12, 10, 7];
const BASE_FREQUENCY: f64 = 220.0;
const CYCLE_SECONDS: u64 = 10;
const SILENT_SECONDS: std::ops::Range<u64> = 6..9;
const LEVEL: f64 = 0.22;

pub fn is_silent_second(second: u64) -> bool {
    SILENT_SECONDS.contains(&(second % CYCLE_SECONDS))
}

fn envelope(time: f64, length: f64) -> f64 {
    let attack = (time / 0.012).min(1.0);
    let release = ((length - time) / 0.06).clamp(0.0, 1.0);
    attack * release
}

pub fn render(first_frame: u64, sample_rate: u32, interleaved: &mut [f32]) {
    let rate = f64::from(sample_rate);
    for (offset, frame) in interleaved.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let index = first_frame + offset as u64;
        let second = index / u64::from(sample_rate);
        if is_silent_second(second) {
            frame.fill(0.0);
            continue;
        }
        let time = (index % u64::from(sample_rate)) as f64 / rate;
        let half = time % 0.5;
        let step = (second * 2 + u64::from(time >= 0.5)) as usize;
        let semitone = SCALE[step % SCALE.len()];
        let root = BASE_FREQUENCY * 2f64.powf(f64::from(semitone) / 12.0);
        let shape = envelope(half, 0.5) * LEVEL;
        let fundamental = (TAU * root * half).sin();
        let octave = (TAU * root * 2.0 * half).sin() * 0.35;
        let fifth = (TAU * root * 1.5 * half).sin();
        let click = if time < 0.02 {
            (TAU * 1800.0 * time).sin() * (1.0 - time / 0.02) * 0.6
        } else {
            0.0
        };
        frame[0] = ((fundamental + octave) * shape + click) as f32;
        frame[1] = ((fifth * 0.8 + fundamental * 0.3) * shape + click) as f32;
    }
}
