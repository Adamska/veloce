use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use ringbuf::traits::{Observer, Producer};

use super::{
    equalizer::Equalizer,
    params::{AudioControls, MAX_SPEED},
    renderer::{Marker, RingWriter},
    silence::SilenceDetector,
    stretcher::TimeStretcher,
};
use crate::{
    engine::{SeekRequest, SessionShared},
    media::{AUDIO_CHANNELS, Micros},
};

pub type SessionSlot = Arc<Mutex<Option<Arc<SessionShared>>>>;

const BLOCK_FRAMES: usize = 512;
const MAX_INPUT_FRAMES: usize = (BLOCK_FRAMES as f32 * MAX_SPEED) as usize + 2;
const MIN_BUFFER_SECONDS: f64 = 0.05;
const GAP_PATIENCE: Micros = 500_000;
const IDLE: Duration = Duration::from_millis(2);

pub struct Processor {
    controls: Arc<AudioControls>,
    slot: SessionSlot,
    ring: RingWriter,
    sample_rate: u32,
    stretcher: TimeStretcher,
    equalizer: Equalizer,
    silence: SilenceDetector,
    input: Vec<f32>,
    output: Vec<f32>,
    epoch: u64,
    cursor: i64,
    primed: bool,
    carry: f64,
    latency: f64,
}

impl Processor {
    pub fn new(
        controls: Arc<AudioControls>,
        slot: SessionSlot,
        ring: RingWriter,
        sample_rate: u32,
    ) -> Self {
        let stretcher = TimeStretcher::new(sample_rate);
        let preroll = stretcher.input_latency() + stretcher.output_latency() * MAX_SPEED as usize;
        Self {
            controls,
            slot,
            ring,
            sample_rate,
            stretcher,
            equalizer: Equalizer::new(sample_rate),
            silence: SilenceDetector::new(sample_rate),
            input: vec![0.0; MAX_INPUT_FRAMES.max(preroll + 2) * AUDIO_CHANNELS],
            output: vec![0.0; BLOCK_FRAMES.max(preroll) * AUDIO_CHANNELS],
            epoch: 0,
            cursor: 0,
            primed: false,
            carry: 0.0,
            latency: 0.0,
        }
    }

    pub fn run(mut self, shutdown: Arc<AtomicBool>) {
        while !shutdown.load(Ordering::Acquire) {
            if !self.step() {
                thread::sleep(IDLE);
            }
        }
    }

    fn resync(&mut self, session: &SessionShared, state: SeekRequest) {
        let delay = self.controls.delay_micros.load(Ordering::Relaxed);
        self.epoch = state.epoch;
        self.controls
            .live_epoch
            .store(state.epoch, Ordering::Release);
        self.cursor = session.audio.frame_of(state.target - delay);
        self.stretcher.reset();
        self.silence.reset();
        self.primed = false;
        self.carry = 0.0;
    }

    fn silence_allowance(&self, session: &SessionShared, cursor: i64) -> Option<usize> {
        if let Some(next) = session.audio.next_start_after(cursor) {
            return Some((next - cursor).max(0) as usize);
        }
        let time = session.audio.time_of(cursor);
        if session.end_of_stream() {
            let tail = (self.latency * 1e6 / f64::from(self.sample_rate)) as Micros;
            return (time < session.end_time() + tail).then_some(usize::MAX);
        }
        let patience = if session.audio_track().is_none() {
            0
        } else {
            GAP_PATIENCE
        };
        let video_is_ahead = session.has_video()
            && session
                .cache
                .span()
                .is_some_and(|span| span.end > time + patience);
        video_is_ahead.then_some(usize::MAX)
    }

    fn fill_input(&mut self, session: &SessionShared, frames: usize) -> bool {
        let mut filled = 0;
        while filled < frames {
            let cursor = self.cursor + filled as i64;
            let range = filled * AUDIO_CHANNELS..frames * AUDIO_CHANNELS;
            filled += session.audio.read(cursor, &mut self.input[range]);
            if filled == frames {
                break;
            }
            let quiet = match self.silence_allowance(session, self.cursor + filled as i64) {
                Some(allowance) if allowance > 0 => allowance.min(frames - filled),
                _ => return false,
            };
            self.input[filled * AUDIO_CHANNELS..(filled + quiet) * AUDIO_CHANNELS].fill(0.0);
            filled += quiet;
        }
        true
    }

    fn prime(&mut self, session: &SessionShared, speed: f32) -> bool {
        let preroll = self.stretcher.input_latency();
        let discard = self.stretcher.output_latency();
        let lead_in = (discard as f32 * speed).round() as usize;
        if !self.fill_input(session, preroll + lead_in) {
            return false;
        }
        let (head, tail) = self.input.split_at(preroll * AUDIO_CHANNELS);
        self.stretcher.prime(head, speed);
        self.stretcher.process(
            &tail[..lead_in * AUDIO_CHANNELS],
            &mut self.output[..discard * AUDIO_CHANNELS],
        );
        self.cursor += (preroll + lead_in) as i64;
        self.latency = self.stretcher.latency_in_source_frames(speed);
        self.primed = true;
        true
    }

    fn step(&mut self) -> bool {
        let Some(session) = self.slot.lock().clone() else {
            self.epoch = 0;
            self.controls.live_epoch.store(0, Ordering::Release);
            return false;
        };
        let state = session.seek_state();
        if state.epoch != self.epoch {
            self.resync(&session, state);
        }
        if !session.can_start_audio(self.epoch) {
            return false;
        }
        let callback = f64::from(self.controls.callback_frames.load(Ordering::Relaxed));
        let wanted = (f64::from(self.sample_rate) * MIN_BUFFER_SECONDS).max(callback * 2.5);
        let queued = self.ring.samples.occupied_len() / AUDIO_CHANNELS;
        if queued as f64 > wanted
            || self.ring.samples.vacant_len() < BLOCK_FRAMES * AUDIO_CHANNELS
            || self.ring.markers.is_full()
        {
            return false;
        }

        let base = self.controls.speed.load();
        let speed = self.silence.effective_speed(&self.controls.silence, base);
        self.controls.effective_speed.store(speed);
        if !self.primed && !self.prime(&session, speed) {
            return false;
        }
        let exact = BLOCK_FRAMES as f64 * f64::from(speed) + self.carry;
        let frames = (exact.floor() as usize).clamp(1, MAX_INPUT_FRAMES);
        self.carry = exact - frames as f64;
        if !self.fill_input(&session, frames) {
            return false;
        }
        let start = self.cursor;
        self.cursor += frames as i64;

        let block = &mut self.input[..frames * AUDIO_CHANNELS];
        self.equalizer.process(&self.controls.equalizer, block);
        self.silence.observe(&self.controls.silence, block);
        let rendered = &mut self.output[..BLOCK_FRAMES * AUDIO_CHANNELS];
        self.stretcher.process(block, rendered);

        let target_latency = self.stretcher.latency_in_source_frames(speed);
        let slew = frames as f64 * 0.1;
        self.latency += (target_latency - self.latency).clamp(-slew, slew);
        let rate = f64::from(self.sample_rate);
        let span = (frames as f64 * 1e6 / rate) as Micros;
        let delay = self.controls.delay_micros.load(Ordering::Relaxed);
        let marker = Marker {
            epoch: self.epoch,
            frames: BLOCK_FRAMES as u32,
            start: session.audio.time_of(start) - (self.latency * 1e6 / rate) as Micros + delay,
            span,
        };
        self.ring.samples.push_slice(rendered);
        let _ = self.ring.markers.try_push(marker);

        if speed > base {
            let saved = span - (span as f64 * f64::from(base / speed)) as Micros;
            session
                .telemetry
                .silence_skipped_micros
                .fetch_add(saved.max(0) as u64, Ordering::Relaxed);
        }
        true
    }
}
