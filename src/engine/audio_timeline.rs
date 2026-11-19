use std::collections::VecDeque;

use parking_lot::RwLock;

use crate::media::{AUDIO_CHANNELS, AudioChunk, MICROS_PER_SECOND, Micros};

const SNAP_TOLERANCE: Micros = 8_000;

struct Segment {
    start: i64,
    samples: Vec<f32>,
}

impl Segment {
    fn end(&self) -> i64 {
        self.start + (self.samples.len() / AUDIO_CHANNELS) as i64
    }
}

pub struct AudioTimeline {
    segments: RwLock<VecDeque<Segment>>,
    sample_rate: u32,
}

impl AudioTimeline {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            segments: RwLock::default(),
            sample_rate: sample_rate.max(1),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frame_of(&self, time: Micros) -> i64 {
        let rate = i128::from(self.sample_rate);
        let scaled = i128::from(time) * rate;
        (scaled.div_euclid(i128::from(MICROS_PER_SECOND))) as i64
    }

    pub fn time_of(&self, frame: i64) -> Micros {
        (i128::from(frame) * i128::from(MICROS_PER_SECOND) / i128::from(self.sample_rate)) as Micros
    }

    pub fn push(&self, chunk: AudioChunk) {
        if chunk.samples.is_empty() {
            return;
        }
        let mut start = self.frame_of(chunk.pts);
        let mut samples = chunk.samples;
        let mut segments = self.segments.write();
        if let Some(last) = segments.back() {
            let end = last.end();
            let tolerance = self.frame_of(SNAP_TOLERANCE);
            if (start - end).abs() <= tolerance {
                start = end;
            } else if start < end {
                let overlap = ((end - start) as usize * AUDIO_CHANNELS).min(samples.len());
                samples.drain(..overlap);
                start = end;
                if samples.is_empty() {
                    return;
                }
            }
        }
        segments.push_back(Segment { start, samples });
    }

    pub fn read(&self, cursor: i64, out: &mut [f32]) -> usize {
        let segments = self.segments.read();
        let mut index = match segments
            .partition_point(|segment| segment.start <= cursor)
            .checked_sub(1)
        {
            Some(index) => index,
            None => return 0,
        };
        let wanted = out.len() / AUDIO_CHANNELS;
        let mut position = cursor;
        let mut written = 0;
        while written < wanted {
            let Some(segment) = segments.get(index) else {
                break;
            };
            if position < segment.start || position >= segment.end() {
                break;
            }
            let offset = (position - segment.start) as usize;
            let count = (wanted - written).min((segment.end() - position) as usize);
            out[written * AUDIO_CHANNELS..(written + count) * AUDIO_CHANNELS].copy_from_slice(
                &segment.samples[offset * AUDIO_CHANNELS..(offset + count) * AUDIO_CHANNELS],
            );
            written += count;
            position += count as i64;
            index += 1;
        }
        written
    }

    pub fn next_start_after(&self, cursor: i64) -> Option<i64> {
        let segments = self.segments.read();
        let index = segments.partition_point(|segment| segment.start <= cursor);
        segments.get(index).map(|segment| segment.start)
    }

    pub fn bounds(&self) -> Option<(i64, i64)> {
        let segments = self.segments.read();
        Some((segments.front()?.start, segments.back()?.end()))
    }

    pub fn end_time(&self) -> Option<Micros> {
        self.bounds().map(|(_, end)| self.time_of(end))
    }

    pub fn covers(&self, time: Micros) -> bool {
        let frame = self.frame_of(time);
        self.bounds()
            .is_some_and(|(start, end)| start <= frame && frame < end)
    }

    pub fn trim(&self, playhead: Micros, behind: Micros) {
        let horizon = self.frame_of(playhead - behind);
        let mut segments = self.segments.write();
        while segments.len() > 1 && segments.front().is_some_and(|first| first.end() < horizon) {
            segments.pop_front();
        }
    }

    pub fn flush(&self) {
        self.segments.write().clear();
    }

    pub fn exchange(&self, other: &Self) {
        if !std::ptr::eq(self, other) {
            std::mem::swap(&mut *self.segments.write(), &mut *other.segments.write());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(pts: Micros, frames: usize, value: f32) -> AudioChunk {
        AudioChunk {
            pts,
            samples: vec![value; frames * AUDIO_CHANNELS],
        }
    }

    #[test]
    fn reads_across_contiguous_chunks_sample_exactly() {
        let timeline = AudioTimeline::new(48_000);
        timeline.push(chunk(0, 480, 1.0));
        timeline.push(chunk(10_000, 480, 2.0));
        let mut out = vec![0.0; 2 * 600];
        assert_eq!(timeline.read(240, &mut out), 600);
        assert_eq!(out[2 * 239], 1.0);
        assert_eq!(out[2 * 240], 2.0);
    }

    #[test]
    fn snaps_rounding_jitter_but_preserves_real_gaps() {
        let timeline = AudioTimeline::new(44_100);
        timeline.push(chunk(0, 1024, 1.0));
        timeline.push(chunk(23_219, 1024, 1.0));
        assert_eq!(timeline.bounds(), Some((0, 2048)));
        timeline.push(chunk(1_000_000, 1024, 1.0));
        let mut out = vec![0.0; 2 * 4096];
        assert_eq!(timeline.read(0, &mut out), 2048);
        assert_eq!(timeline.next_start_after(2048), Some(44_100));
    }

    #[test]
    fn overlapping_chunks_do_not_duplicate_samples() {
        let timeline = AudioTimeline::new(48_000);
        timeline.push(chunk(0, 4800, 1.0));
        timeline.push(chunk(50_000, 4800, 2.0));
        assert_eq!(timeline.bounds(), Some((0, 7200)));
    }

    #[test]
    fn coverage_and_trim_follow_the_playhead() {
        let timeline = AudioTimeline::new(48_000);
        for index in 0..100 {
            timeline.push(chunk(index * 100_000, 4800, 0.5));
        }
        assert!(timeline.covers(9_950_000));
        assert!(!timeline.covers(10_000_000));
        timeline.trim(9_000_000, 2_000_000);
        assert!(!timeline.covers(6_800_000));
        assert!(timeline.covers(7_000_000));
        timeline.flush();
        assert_eq!(timeline.bounds(), None);
    }
}
