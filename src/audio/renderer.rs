use std::sync::{Arc, atomic::Ordering};

use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Split},
};

use super::params::AudioControls;
use crate::{
    engine::{Anchor, ClockAnchor, monotonic_nanos},
    media::{AUDIO_CHANNELS, Micros},
};

const MARKER_CAPACITY: usize = 1024;
const SCRATCH_FRAMES: usize = 1024;
const CLIP_KNEE: f32 = 0.85;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marker {
    pub epoch: u64,
    pub frames: u32,
    pub start: Micros,
    pub span: Micros,
}

impl Marker {
    fn media_at(&self, frame: u32) -> Micros {
        self.start + self.span * Micros::from(frame) / Micros::from(self.frames.max(1))
    }
}

pub struct RingWriter {
    pub samples: HeapProd<f32>,
    pub markers: HeapProd<Marker>,
}

pub struct Renderer {
    samples: HeapCons<f32>,
    markers: HeapCons<Marker>,
    controls: Arc<AudioControls>,
    anchor: Arc<ClockAnchor>,
    sample_rate: u32,
    current: Option<(Marker, u32)>,
    gain: f32,
    last: Option<(u64, Micros)>,
}

pub fn ring(
    sample_rate: u32,
    controls: Arc<AudioControls>,
    anchor: Arc<ClockAnchor>,
) -> (RingWriter, Renderer) {
    let (sample_writer, samples) = HeapRb::new(sample_rate as usize * AUDIO_CHANNELS).split();
    let (marker_writer, markers) = HeapRb::new(MARKER_CAPACITY).split();
    let writer = RingWriter {
        samples: sample_writer,
        markers: marker_writer,
    };
    let renderer = Renderer {
        samples,
        markers,
        gain: controls.gain(),
        controls,
        anchor,
        sample_rate,
        current: None,
        last: None,
    };
    (writer, renderer)
}

#[inline]
fn soft_clip(sample: f32) -> f32 {
    let magnitude = sample.abs();
    if magnitude <= CLIP_KNEE {
        return sample;
    }
    let headroom = 1.0 - CLIP_KNEE;
    (CLIP_KNEE + headroom * ((magnitude - CLIP_KNEE) / headroom).tanh()).copysign(sample)
}

impl Renderer {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn next_marker(&mut self, live: u64) -> Option<(Marker, u32)> {
        if let Some((marker, consumed)) = self.current {
            if marker.epoch == live {
                return self.current;
            }
            self.samples
                .skip((marker.frames - consumed) as usize * AUDIO_CHANNELS);
            self.current = None;
        }
        while let Some(marker) = self.markers.try_pop() {
            if marker.epoch == live {
                self.current = Some((marker, 0));
                return self.current;
            }
            self.samples.skip(marker.frames as usize * AUDIO_CHANNELS);
        }
        None
    }

    fn freeze(&self) {
        let (epoch, media) = self.last.unwrap_or((0, 0));
        self.anchor.publish(Anchor {
            epoch,
            media,
            at_nanos: monotonic_nanos(),
            rate: 0.0,
            limit: media,
        });
    }

    pub fn render(&mut self, out: &mut [f32], channels: usize, latency_nanos: u64) {
        let frames = out.len() / channels.max(1);
        self.controls
            .callback_frames
            .store(frames as u32, Ordering::Relaxed);
        let live = self.controls.live_epoch.load(Ordering::Acquire);
        let target_gain = self.controls.gain();
        if self.controls.paused.load(Ordering::Acquire) {
            out.fill(0.0);
            self.gain = target_gain;
            self.anchor.publish(Anchor::default());
            return;
        }

        let gain_step = (target_gain - self.gain) / frames.max(1) as f32;
        let mut scratch = [0.0f32; SCRATCH_FRAMES * AUDIO_CHANNELS];
        let mut written = 0;
        let mut first = None;
        let mut limit = 0;
        while written < frames {
            let Some((marker, consumed)) = self.next_marker(live) else {
                break;
            };
            let take = (frames - written)
                .min((marker.frames - consumed) as usize)
                .min(SCRATCH_FRAMES);
            let got = self
                .samples
                .pop_slice(&mut scratch[..take * AUDIO_CHANNELS])
                / AUDIO_CHANNELS;
            if got == 0 {
                self.current = None;
                break;
            }
            if first.is_none() {
                let seconds = f64::from(marker.frames) / f64::from(self.sample_rate);
                let rate = (marker.span as f64 / 1e6 / seconds) as f32;
                first = Some((marker.epoch, marker.media_at(consumed), rate));
            }
            for (index, frame) in scratch[..got * AUDIO_CHANNELS]
                .as_chunks::<AUDIO_CHANNELS>()
                .0
                .iter()
                .enumerate()
            {
                self.gain += gain_step;
                let left = soft_clip(frame[0] * self.gain);
                let right = soft_clip(frame[1] * self.gain);
                let target = &mut out[(written + index) * channels..][..channels];
                match target {
                    [mono] => *mono = 0.5 * (left + right),
                    [first, second, rest @ ..] => {
                        *first = left;
                        *second = right;
                        rest.fill(0.0);
                    }
                    [] => {}
                }
            }
            written += got;
            let consumed = consumed + got as u32;
            limit = marker.media_at(consumed);
            self.last = Some((marker.epoch, limit));
            self.current = (consumed < marker.frames).then_some((marker, consumed));
        }
        out[written * channels..].fill(0.0);
        self.gain = target_gain;

        match first {
            Some((epoch, media, rate)) => self.anchor.publish(Anchor {
                epoch,
                media,
                at_nanos: monotonic_nanos() + latency_nanos,
                rate,
                limit,
            }),
            None => {
                self.controls.underruns.fetch_add(1, Ordering::Relaxed);
                self.freeze();
            }
        }
    }
}
