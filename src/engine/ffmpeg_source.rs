use std::collections::VecDeque;

use anyhow::Result;
use ffmpeg_next as ffmpeg;

use super::{
    audio_decoder::AudioDecoder, decoder::VideoDecoder, demuxer::Demuxer,
    subtitle_decoder::SubtitleDecoder,
};
use crate::media::{MediaInfo, MediaSource, Micros, SourceEvent};

const PREROLL_KEEP: Micros = 250_000;

#[derive(Clone, Copy, Debug)]
pub struct OpenOptions {
    pub sample_rate: u32,
    pub norm16_textures: bool,
}

pub struct FfmpegSource {
    demuxer: Demuxer,
    video: Option<VideoDecoder>,
    audio: Option<(usize, AudioDecoder)>,
    audio_track: Option<usize>,
    subtitles: Vec<SubtitleDecoder>,
    packet: ffmpeg::Packet,
    queue: VecDeque<SourceEvent>,
    options: OpenOptions,
    skip_before: Micros,
    finished: bool,
}

pub fn initialize() -> Result<()> {
    ffmpeg::init()?;
    ffmpeg::format::network::init();
    ffmpeg::log::set_level(ffmpeg::log::Level::Error);
    Ok(())
}

impl FfmpegSource {
    pub fn open(
        location: &str,
        options: OpenOptions,
        cancelled: impl FnMut() -> bool + Send + 'static,
    ) -> Result<Self> {
        let mut demuxer = Demuxer::open(location, cancelled)?;
        let start_offset = demuxer.start_offset;
        let video = match demuxer.video_stream.and_then(|index| demuxer.stream(index)) {
            Some(stream) => {
                let rate = demuxer
                    .info
                    .video
                    .as_ref()
                    .map_or(0.0, |video| video.frame_rate);
                match VideoDecoder::open(&stream, start_offset, rate, options.norm16_textures) {
                    Ok(decoder) => Some(decoder),
                    Err(error) => {
                        log::warn!("video stream disabled: {error:#}");
                        None
                    }
                }
            }
            None => None,
        };
        if video.is_none() {
            demuxer.video_stream = None;
            demuxer.info.video = None;
        }
        let subtitles = demuxer
            .subtitle_streams
            .iter()
            .enumerate()
            .filter(|(track, _)| demuxer.info.subtitle_tracks[*track].text_based)
            .filter_map(|(track, index)| {
                let stream = demuxer.stream(*index)?;
                SubtitleDecoder::open(&stream, track, start_offset).ok()
            })
            .collect();
        let mut source = Self {
            demuxer,
            video,
            audio: None,
            audio_track: None,
            subtitles,
            packet: ffmpeg::Packet::empty(),
            queue: VecDeque::new(),
            options,
            skip_before: Micros::MIN,
            finished: false,
        };
        let default_audio = source.demuxer.default_audio;
        if let Err(error) = source.select_audio_track(default_audio) {
            log::warn!("audio disabled: {error:#}");
        }
        Ok(source)
    }

    fn drain(&mut self) -> Result<()> {
        if let Some(video) = self.video.as_mut() {
            video.drain(&mut self.queue)?;
        }
        if let Some((_, audio)) = self.audio.as_mut() {
            audio.drain(&mut self.queue)?;
        }
        Ok(())
    }

    fn reposition(&mut self, target: Micros) -> Result<()> {
        self.demuxer.seek(target)?;
        if let Some(video) = self.video.as_mut() {
            video.flush();
        }
        if let Some((_, audio)) = self.audio.as_mut() {
            audio.flush();
        }
        for subtitle in &mut self.subtitles {
            subtitle.flush();
        }
        Ok(())
    }

    fn apply_skip(&mut self) {
        if let Some(video) = self.video.as_mut() {
            video.skip_before(self.skip_before);
        }
        if let Some((_, audio)) = self.audio.as_mut() {
            audio.skip_before(self.skip_before);
        }
    }
}

impl MediaSource for FfmpegSource {
    fn info(&self) -> &MediaInfo {
        &self.demuxer.info
    }

    fn decoder_backend(&self) -> String {
        self.video
            .as_ref()
            .map_or_else(|| "Audio only".to_owned(), VideoDecoder::backend)
    }

    fn seek(&mut self, target: Micros) -> Result<()> {
        let rolling = !self.finished
            && self.video.is_some()
            && self
                .demuxer
                .keyframes
                .reachable_by_decoding_forward(self.demuxer.position(), target);
        self.queue.clear();
        self.finished = false;
        self.skip_before = target - PREROLL_KEEP;
        if !rolling {
            self.reposition(target)?;
        }
        self.apply_skip();
        Ok(())
    }

    fn selected_audio_track(&self) -> Option<usize> {
        self.audio_track
    }

    fn select_audio_track(&mut self, track: Option<usize>) -> Result<()> {
        self.audio = None;
        self.audio_track = None;
        let Some(index) = track.and_then(|track| self.demuxer.audio_streams.get(track).copied())
        else {
            return Ok(());
        };
        let Some(stream) = self.demuxer.stream(index) else {
            return Ok(());
        };
        let mut decoder =
            AudioDecoder::open(&stream, self.demuxer.start_offset, self.options.sample_rate)?;
        decoder.skip_before(self.skip_before);
        self.audio = Some((index, decoder));
        self.audio_track = track;
        Ok(())
    }

    fn next_event(&mut self) -> Result<SourceEvent> {
        if let Some(event) = self.queue.pop_front() {
            return Ok(event);
        }
        if self.finished {
            return Ok(SourceEvent::EndOfStream);
        }
        if !self.demuxer.read(&mut self.packet)? {
            self.finished = true;
            self.drain()?;
            return Ok(self.queue.pop_front().unwrap_or(SourceEvent::EndOfStream));
        }
        let stream = self.packet.stream();
        if Some(stream) == self.demuxer.video_stream {
            if let Some(video) = self.video.as_mut()
                && video.send(&self.packet, &mut self.queue)?
            {
                let position = self.demuxer.position();
                self.reposition(position)?;
                self.apply_skip();
            }
        } else if let Some((index, audio)) = self.audio.as_mut()
            && *index == stream
        {
            audio.send(&self.packet, &mut self.queue)?;
        } else if let Some(subtitle) = self
            .subtitles
            .iter_mut()
            .find(|decoder| decoder.stream_index == stream)
        {
            subtitle.decode(&self.packet, &mut self.queue)?;
        }
        Ok(self.queue.pop_front().unwrap_or(SourceEvent::Idle))
    }
}
