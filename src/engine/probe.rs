use std::path::Path;

use ffmpeg_next::{self as ffmpeg, Rational, Rescale, ffi, format, rescale};

use super::subtitle_decoder::is_text_codec;
use crate::media::{
    AudioTrackInfo, Chapter, MediaInfo, Micros, SubtitleTrackInfo, VideoStreamInfo,
};

pub struct Layout<'a> {
    pub video: Option<&'a ffmpeg::Stream<'a>>,
    pub audio_streams: &'a [usize],
    pub subtitle_streams: &'a [usize],
    pub start_offset: Micros,
    pub duration: Option<Micros>,
}

fn positive(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|value| *value > 0)
}

fn rational(value: Rational) -> f64 {
    if value.denominator() == 0 {
        0.0
    } else {
        f64::from(value.numerator()) / f64::from(value.denominator())
    }
}

pub fn describe(input: &format::context::Input, location: &str, layout: &Layout<'_>) -> MediaInfo {
    let tag = |stream: &ffmpeg::Stream, key: &str| stream.metadata().get(key).map(str::to_owned);
    let video_info = layout.video.map(|stream| {
        let parameters = stream.parameters();
        // SAFETY: the parameters pointer is owned by the stream.
        let (width, height, format) = unsafe {
            let raw = &*parameters.as_ptr();
            (raw.width, raw.height, raw.format)
        };
        let pixel_format =
            format::Pixel::from(unsafe { std::mem::transmute::<i32, ffi::AVPixelFormat>(format) });
        let rate = match rational(stream.avg_frame_rate()) {
            rate if rate > 0.0 => rate,
            _ => rational(stream.rate()),
        };
        VideoStreamInfo {
            codec: parameters.id().name().to_owned(),
            pixel_format: format!("{pixel_format:?}").to_lowercase(),
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            frame_rate: rate,
            bit_rate: positive(parameters.bit_rate()),
        }
    });
    let audio_tracks = layout
        .audio_streams
        .iter()
        .filter_map(|index| input.stream(*index))
        .map(|stream| {
            let parameters = stream.parameters();
            // SAFETY: the parameters pointer is owned by the stream.
            let raw = unsafe { &*parameters.as_ptr() };
            AudioTrackInfo {
                codec: parameters.id().name().to_owned(),
                channels: raw.ch_layout.nb_channels.max(0) as u16,
                sample_rate: raw.sample_rate.max(0) as u32,
                bit_rate: positive(parameters.bit_rate()),
                language: tag(&stream, "language"),
                title: tag(&stream, "title"),
            }
        })
        .collect();
    let subtitle_tracks = layout
        .subtitle_streams
        .iter()
        .filter_map(|index| input.stream(*index))
        .map(|stream| SubtitleTrackInfo {
            codec: stream.parameters().id().name().to_owned(),
            language: tag(&stream, "language"),
            title: tag(&stream, "title"),
            text_based: is_text_codec(stream.parameters().id()),
        })
        .collect();
    let chapters = input
        .chapters()
        .enumerate()
        .map(|(index, chapter)| {
            let scale = |value: i64| {
                value.rescale(chapter.time_base(), rescale::TIME_BASE) - layout.start_offset
            };
            Chapter {
                start: scale(chapter.start()),
                end: scale(chapter.end()),
                title: chapter
                    .metadata()
                    .get("title")
                    .map_or_else(|| format!("Chapter {}", index + 1), str::to_owned),
            }
        })
        .collect();
    let fallback_title = Path::new(location.split('?').next().unwrap_or(location))
        .file_name()
        .map_or_else(
            || location.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
    MediaInfo {
        title: input
            .metadata()
            .get("title")
            .filter(|title| !title.trim().is_empty())
            .map_or(fallback_title, str::to_owned),
        location: location.to_owned(),
        container: input.format().description().to_owned(),
        duration: layout.duration,
        seekable: layout.duration.is_some(),
        bit_rate: positive(input.bit_rate()),
        video: video_info,
        audio_tracks,
        subtitle_tracks,
        chapters,
    }
}
