use std::sync::Arc;

use anyhow::Result;

use super::{pattern, tone};
use crate::media::{
    AudioChunk, AudioTrackInfo, BufferPool, Chapter, ColorInfo, ColorMatrix, MICROS_PER_SECOND,
    MediaInfo, MediaSource, Micros, PixelLayout, PooledPlanes, SourceEvent, Transfer, VideoFrame,
    VideoStreamInfo,
};

pub const WIDTH: u32 = 3840;
pub const HEIGHT: u32 = 2160;
pub const FRAME_RATE: u32 = 60;
pub const DURATION_SECONDS: u64 = 600;
pub const TITLE: &str = "Veloce 4K Scrubbing Demo";

const CHAPTER_SECONDS: u64 = 60;
const TOTAL_FRAMES: u64 = DURATION_SECONDS * FRAME_RATE as u64;

pub struct SyntheticSource {
    info: MediaInfo,
    pool: Arc<BufferPool>,
    sample_rate: u32,
    next_frame: u64,
    audio_cursor: u64,
    audio_enabled: bool,
}

fn frame_pts(index: u64) -> Micros {
    (index * MICROS_PER_SECOND as u64 / u64::from(FRAME_RATE)) as Micros
}

impl SyntheticSource {
    pub fn new(sample_rate: u32) -> Self {
        let chapters = (0..DURATION_SECONDS / CHAPTER_SECONDS)
            .map(|index| Chapter {
                start: (index * CHAPTER_SECONDS) as Micros * MICROS_PER_SECOND,
                end: ((index + 1) * CHAPTER_SECONDS) as Micros * MICROS_PER_SECOND,
                title: format!("Segment {:02}", index + 1),
            })
            .collect();
        let info = MediaInfo {
            title: TITLE.to_owned(),
            location: "veloce://demo".to_owned(),
            container: "procedural".to_owned(),
            duration: Some(DURATION_SECONDS as Micros * MICROS_PER_SECOND),
            seekable: true,
            bit_rate: None,
            video: Some(VideoStreamInfo {
                codec: "procedural".to_owned(),
                pixel_format: "nv12".to_owned(),
                width: WIDTH,
                height: HEIGHT,
                frame_rate: f64::from(FRAME_RATE),
                bit_rate: None,
            }),
            audio_tracks: vec![AudioTrackInfo {
                codec: "pcm_f32".to_owned(),
                channels: 2,
                sample_rate,
                bit_rate: None,
                language: None,
                title: Some("Multi-tone sequence".to_owned()),
            }],
            subtitle_tracks: Vec::new(),
            chapters,
        };
        Self {
            info,
            pool: Arc::default(),
            sample_rate,
            next_frame: 0,
            audio_cursor: 0,
            audio_enabled: true,
        }
    }

    fn color() -> ColorInfo {
        ColorInfo {
            matrix: ColorMatrix::Bt709,
            transfer: Transfer::Sdr,
            full_range: false,
        }
    }

    fn audio_frames_until(&self, frame: u64) -> u64 {
        frame * u64::from(self.sample_rate) / u64::from(FRAME_RATE)
    }

    fn video_frame(&mut self) -> VideoFrame {
        let index = self.next_frame;
        self.next_frame += 1;
        let mut planes = PooledPlanes::new(&self.pool, PixelLayout::Nv12, WIDTH, HEIGHT);
        let position = pattern::FramePosition {
            index,
            frame_rate: FRAME_RATE,
            silent: tone::is_silent_second(index / u64::from(FRAME_RATE)),
        };
        pattern::paint(&mut planes, WIDTH, HEIGHT, &position);
        VideoFrame {
            pts: frame_pts(index),
            duration: frame_pts(index + 1) - frame_pts(index),
            width: WIDTH,
            height: HEIGHT,
            pixel_aspect: 1.0,
            layout: PixelLayout::Nv12,
            color: Self::color(),
            interlaced: false,
            storage: Box::new(planes),
        }
    }

    fn audio_chunk(&mut self, until: u64) -> AudioChunk {
        let first = self.audio_cursor;
        let mut samples = vec![0.0; (until - first) as usize * 2];
        tone::render(first, self.sample_rate, &mut samples);
        self.audio_cursor = until;
        AudioChunk {
            pts: (first * MICROS_PER_SECOND as u64 / u64::from(self.sample_rate)) as Micros,
            samples,
        }
    }
}

impl MediaSource for SyntheticSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn decoder_backend(&self) -> String {
        "Procedural generator".to_owned()
    }

    fn seek(&mut self, target: Micros) -> Result<()> {
        let frame = target.max(0) as u64 * u64::from(FRAME_RATE) / MICROS_PER_SECOND as u64;
        self.next_frame = frame.min(TOTAL_FRAMES.saturating_sub(1));
        self.audio_cursor = self.audio_frames_until(self.next_frame);
        Ok(())
    }

    fn selected_audio_track(&self) -> Option<usize> {
        self.audio_enabled.then_some(0)
    }

    fn select_audio_track(&mut self, track: Option<usize>) -> Result<()> {
        self.audio_enabled = track.is_some();
        Ok(())
    }

    fn next_event(&mut self) -> Result<SourceEvent> {
        if self.next_frame >= TOTAL_FRAMES {
            return Ok(SourceEvent::EndOfStream);
        }
        let audio_target = self.audio_frames_until(self.next_frame + 1);
        if self.audio_enabled && self.audio_cursor < audio_target {
            return Ok(SourceEvent::Audio(self.audio_chunk(audio_target)));
        }
        self.audio_cursor = audio_target;
        Ok(SourceEvent::Video(self.video_frame()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next_video(source: &mut SyntheticSource) -> VideoFrame {
        loop {
            match source.next_event() {
                Ok(SourceEvent::Video(frame)) => return frame,
                Ok(SourceEvent::EndOfStream) | Err(_) => panic!("stream ended unexpectedly"),
                Ok(_) => {}
            }
        }
    }

    #[test]
    fn produces_4k_nv12_frames_at_sixty_hertz() {
        let mut source = SyntheticSource::new(48_000);
        let first = next_video(&mut source);
        let second = next_video(&mut source);
        assert_eq!((first.width, first.height), (WIDTH, HEIGHT));
        assert_eq!(first.layout, PixelLayout::Nv12);
        assert_eq!(first.plane(0).data.len(), (WIDTH * HEIGHT) as usize);
        assert_eq!(first.plane(1).data.len(), (WIDTH * HEIGHT / 2) as usize);
        assert_eq!(second.pts - first.pts, 16_666);
    }

    #[test]
    fn seek_lands_on_the_frame_containing_the_target() {
        let mut source = SyntheticSource::new(48_000);
        source.seek(125 * MICROS_PER_SECOND + 20_000).unwrap();
        let frame = next_video(&mut source);
        assert!(frame.pts <= 125 * MICROS_PER_SECOND + 20_000);
        assert!(frame.end() > 125 * MICROS_PER_SECOND + 20_000);
    }

    #[test]
    fn frames_are_deterministic_for_a_given_position() {
        let mut source = SyntheticSource::new(48_000);
        source.seek(42 * MICROS_PER_SECOND).unwrap();
        let first = next_video(&mut source);
        source.seek(42 * MICROS_PER_SECOND).unwrap();
        let again = next_video(&mut source);
        assert_eq!(first.pts, again.pts);
        assert!(first.plane(0).data == again.plane(0).data);
        assert!(first.plane(1).data == again.plane(1).data);
    }

    #[test]
    fn audio_is_gapless_and_matches_the_sample_rate() {
        let mut source = SyntheticSource::new(44_100);
        let mut frames = 0u64;
        let mut expected_pts = 0;
        while frames < 44_100 {
            if let Ok(SourceEvent::Audio(chunk)) = source.next_event() {
                assert!((chunk.pts - expected_pts).abs() <= 1);
                frames += chunk.frames() as u64;
                expected_pts = (frames * 1_000_000 / 44_100) as Micros;
            }
        }
        assert_eq!(frames, 44_100);
    }

    #[test]
    fn silent_windows_carry_no_energy() {
        let mut samples = vec![1.0f32; 2 * 4800];
        tone::render(7 * 48_000, 48_000, &mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));
        tone::render(48_000, 48_000, &mut samples);
        assert!(samples.iter().any(|sample| sample.abs() > 0.05));
    }

    #[test]
    fn stream_ends_after_the_last_frame() {
        let mut source = SyntheticSource::new(48_000);
        source
            .seek(DURATION_SECONDS as Micros * MICROS_PER_SECOND)
            .unwrap();
        let last = next_video(&mut source);
        assert_eq!(last.end(), DURATION_SECONDS as Micros * MICROS_PER_SECOND);
        assert!(matches!(source.next_event(), Ok(SourceEvent::EndOfStream)));
    }
}
