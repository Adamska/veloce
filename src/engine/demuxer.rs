use anyhow::{Context as _, Result};
use ffmpeg_next::{
    self as ffmpeg, Dictionary, Rational, Rescale, ffi,
    format::{self, stream::Disposition},
    media, rescale,
};

use super::{keyframes::KeyframeIndex, probe};
use crate::media::{MediaInfo, Micros};

const NETWORK_TIMEOUT_MICROS: &str = "15000000";

pub struct Demuxer {
    input: format::context::Input,
    pub info: MediaInfo,
    pub video_stream: Option<usize>,
    pub audio_streams: Vec<usize>,
    pub subtitle_streams: Vec<usize>,
    pub default_audio: Option<usize>,
    pub start_offset: Micros,
    pub keyframes: KeyframeIndex,
    video_time_base: Rational,
    position: Micros,
}

pub fn is_network_location(location: &str) -> bool {
    [
        "http://", "https://", "rtsp://", "rtsps://", "rtmp://", "rtp://", "udp://", "tcp://",
        "srt://",
    ]
    .iter()
    .any(|scheme| location.to_ascii_lowercase().starts_with(scheme))
}

fn open_options(location: &str) -> Dictionary<'static> {
    let mut options = Dictionary::new();
    if is_network_location(location) {
        options.set("rw_timeout", NETWORK_TIMEOUT_MICROS);
        options.set("reconnect", "1");
        options.set("reconnect_streamed", "1");
        if location.to_ascii_lowercase().starts_with("rtsp") {
            options.set("rtsp_transport", "tcp");
        }
    }
    options
}

fn container_keyframes(
    stream: &ffmpeg::Stream,
    start_offset: Micros,
    duration: Option<Micros>,
) -> KeyframeIndex {
    let raw = unsafe { stream.as_ptr() };
    // SAFETY: the stream belongs to an open input and index entries are copied out immediately.
    let times = unsafe {
        (0..ffi::avformat_index_get_entries_count(raw))
            .filter_map(|index| ffi::avformat_index_get_entry(raw.cast_mut(), index).as_ref())
            .filter(|entry| entry.flags() & ffi::AVINDEX_KEYFRAME != 0)
            .map(|entry| {
                entry
                    .timestamp
                    .rescale(stream.time_base(), rescale::TIME_BASE)
                    - start_offset
            })
            .collect()
    };
    KeyframeIndex::from_container(times, duration)
}

impl Demuxer {
    pub fn open(location: &str, cancelled: impl FnMut() -> bool + Send + 'static) -> Result<Self> {
        let input = format::input_with_interrupt_and_dictionary(
            &location,
            cancelled,
            open_options(location),
        )
        .with_context(|| format!("failed to open {location}"))?;

        // SAFETY: the format context is valid while `input` is alive.
        let raw_start = unsafe { (*input.as_ptr()).start_time };
        let start_offset = if raw_start == ffi::AV_NOPTS_VALUE {
            0
        } else {
            raw_start
        };
        let duration = Some(input.duration()).filter(|duration| *duration > 0);

        let is_picture =
            |stream: &ffmpeg::Stream| stream.disposition().contains(Disposition::ATTACHED_PIC);
        let of_kind = |kind: media::Type| {
            input
                .streams()
                .filter(move |stream| stream.parameters().medium() == kind)
        };
        let video = input
            .streams()
            .best(media::Type::Video)
            .filter(|stream| !is_picture(stream))
            .or_else(|| of_kind(media::Type::Video).find(|stream| !is_picture(stream)));
        let audio_streams: Vec<usize> = of_kind(media::Type::Audio).map(|s| s.index()).collect();
        let subtitle_streams: Vec<usize> =
            of_kind(media::Type::Subtitle).map(|s| s.index()).collect();
        let default_audio = input
            .streams()
            .best(media::Type::Audio)
            .and_then(|best| {
                audio_streams
                    .iter()
                    .position(|index| *index == best.index())
            })
            .or((!audio_streams.is_empty()).then_some(0));

        let info = probe::describe(
            &input,
            location,
            &probe::Layout {
                video: video.as_ref(),
                audio_streams: &audio_streams,
                subtitle_streams: &subtitle_streams,
                start_offset,
                duration,
            },
        );
        Ok(Self {
            keyframes: video
                .as_ref()
                .map(|stream| container_keyframes(stream, start_offset, duration))
                .unwrap_or_default(),
            video_time_base: video
                .as_ref()
                .map_or(Rational(1, 1_000_000), |s| s.time_base()),
            video_stream: video.map(|stream| stream.index()),
            input,
            info,
            audio_streams,
            subtitle_streams,
            default_audio,
            start_offset,
            position: 0,
        })
    }

    pub fn stream(&self, index: usize) -> Option<ffmpeg::Stream<'_>> {
        self.input.stream(index)
    }

    pub fn position(&self) -> Micros {
        self.position
    }

    pub fn read(&mut self, packet: &mut ffmpeg::Packet) -> Result<bool> {
        match packet.read(&mut self.input) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => return Ok(false),
            Err(error) => return Err(error).context("failed to read from the container"),
        }
        if Some(packet.stream()) == self.video_stream
            && let Some(timestamp) = packet.pts().or(packet.dts())
        {
            self.position =
                timestamp.rescale(self.video_time_base, rescale::TIME_BASE) - self.start_offset;
        }
        Ok(true)
    }

    pub fn seek(&mut self, target: Micros) -> Result<()> {
        let target = target.max(0);
        let anchor = self.keyframes.seek_anchor(target) + self.start_offset;
        self.input
            .seek(anchor, ..anchor)
            .or_else(|_| self.input.seek(anchor, ..))
            .with_context(|| format!("the container rejected a seek to {target}"))?;
        self.position = target;
        if !self.keyframes.is_complete()
            && let Some(stream) = self.video_stream.and_then(|index| self.input.stream(index))
        {
            self.keyframes = container_keyframes(&stream, self.start_offset, self.info.duration);
        }
        Ok(())
    }
}
