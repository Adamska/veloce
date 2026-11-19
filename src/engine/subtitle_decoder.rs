use std::collections::VecDeque;

use anyhow::{Context as _, Result};
use ffmpeg_next::{
    self as ffmpeg, Rational, Rescale, codec, decoder, ffi, rescale, subtitle::Rect,
};

use crate::media::{Micros, RawSubtitle, SourceEvent, SubtitlePayload};

const FALLBACK_DURATION: Micros = 4_000_000;

pub fn is_text_codec(id: codec::Id) -> bool {
    matches!(
        id,
        codec::Id::SUBRIP
            | codec::Id::SRT
            | codec::Id::ASS
            | codec::Id::SSA
            | codec::Id::WEBVTT
            | codec::Id::MOV_TEXT
            | codec::Id::TEXT
    )
}

pub struct SubtitleDecoder {
    pub stream_index: usize,
    track: usize,
    decoder: decoder::Subtitle,
    time_base: Rational,
    start_offset: Micros,
}

impl SubtitleDecoder {
    pub fn open(stream: &ffmpeg::Stream, track: usize, start_offset: Micros) -> Result<Self> {
        let decoder = codec::Context::from_parameters(stream.parameters())
            .context("unsupported subtitle codec parameters")?
            .decoder()
            .subtitle()
            .context("failed to open the subtitle decoder")?;
        Ok(Self {
            stream_index: stream.index(),
            track,
            decoder,
            time_base: stream.time_base(),
            start_offset,
        })
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    pub fn decode(
        &mut self,
        packet: &ffmpeg::Packet,
        out: &mut VecDeque<SourceEvent>,
    ) -> Result<()> {
        let mut subtitle = ffmpeg::Subtitle::new();
        if !self.decoder.decode(packet, &mut subtitle)? {
            return Ok(());
        }
        let to_micros = |value: i64| value.rescale(self.time_base, rescale::TIME_BASE);
        let base = to_micros(packet.pts().or(packet.dts()).unwrap_or(0)) - self.start_offset;
        let start = base + Micros::from(subtitle.start()) * 1_000;
        let end = match (subtitle.end(), packet.duration()) {
            (shown, _) if shown > 0 && shown != u32::MAX => base + Micros::from(shown) * 1_000,
            (_, duration) if duration > 0 => base + to_micros(duration),
            _ => start + FALLBACK_DURATION,
        };
        for rect in subtitle.rects() {
            let payload = match rect {
                Rect::Text(text) => SubtitlePayload::Plain(text.get().to_owned()),
                Rect::Ass(ass) => SubtitlePayload::AssDialogue(ass.get().to_owned()),
                _ => continue,
            };
            out.push_back(SourceEvent::Subtitle(RawSubtitle {
                track: self.track,
                start,
                end,
                payload,
            }));
        }
        // SAFETY: the subtitle was filled by the decoder and its rectangles are released once.
        unsafe { ffi::avsubtitle_free(subtitle.as_mut_ptr()) };
        Ok(())
    }
}
