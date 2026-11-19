use std::collections::VecDeque;

use anyhow::{Context as _, Result};
use ffmpeg_next::{
    self as ffmpeg, ChannelLayout, Rational, Rescale, codec, decoder,
    format::{Sample, sample::Type as SampleType},
    frame, rescale,
    software::resampling,
};

use crate::media::{AUDIO_CHANNELS, AudioChunk, MICROS_PER_SECOND, Micros, SourceEvent};

const DISCONTINUITY: Micros = 100_000;
const RESAMPLER_HEADROOM: usize = 64;

type ResamplerKey = (Sample, i32, u32);

pub struct AudioDecoder {
    decoder: decoder::Audio,
    resampler: Option<(resampling::Context, ResamplerKey)>,
    time_base: Rational,
    start_offset: Micros,
    output_rate: u32,
    base_pts: Micros,
    emitted: u64,
    synced: bool,
    skip_before: Micros,
    decoded: frame::Audio,
}

impl AudioDecoder {
    pub fn open(stream: &ffmpeg::Stream, start_offset: Micros, output_rate: u32) -> Result<Self> {
        let decoder = codec::Context::from_parameters(stream.parameters())
            .context("unsupported audio codec parameters")?
            .decoder()
            .audio()
            .context("failed to open the audio decoder")?;
        Ok(Self {
            decoder,
            resampler: None,
            time_base: stream.time_base(),
            start_offset,
            output_rate,
            base_pts: 0,
            emitted: 0,
            synced: false,
            skip_before: Micros::MIN,
            decoded: frame::Audio::empty(),
        })
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        self.synced = false;
    }

    pub fn skip_before(&mut self, threshold: Micros) {
        self.skip_before = threshold;
    }

    fn next_pts(&self) -> Micros {
        self.base_pts
            + (self.emitted * MICROS_PER_SECOND as u64 / u64::from(self.output_rate)) as Micros
    }

    pub fn send(&mut self, packet: &ffmpeg::Packet, out: &mut VecDeque<SourceEvent>) -> Result<()> {
        match self.decoder.send_packet(packet) {
            Ok(()) | Err(ffmpeg::Error::InvalidData) => self.receive(out),
            Err(error) => Err(error).context("audio packet rejected"),
        }
    }

    pub fn drain(&mut self, out: &mut VecDeque<SourceEvent>) -> Result<()> {
        let _ = self.decoder.send_eof();
        self.receive(out)
    }

    fn receive(&mut self, out: &mut VecDeque<SourceEvent>) -> Result<()> {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            if self.decoded.samples() == 0 {
                continue;
            }
            if self.decoded.channel_layout().is_empty() {
                let channels = i32::from(self.decoded.channels());
                self.decoded
                    .set_channel_layout(ChannelLayout::default(channels));
            }
            if let Some(timestamp) = self.decoded.timestamp().or(self.decoded.pts()) {
                let pts = timestamp.rescale(self.time_base, rescale::TIME_BASE) - self.start_offset;
                if !self.synced || (pts - self.next_pts()).abs() > DISCONTINUITY {
                    self.base_pts = pts;
                    self.emitted = 0;
                    self.synced = true;
                }
            }
            let key = (
                self.decoded.format(),
                self.decoded.channel_layout().channels(),
                self.decoded.rate(),
            );
            if self
                .resampler
                .as_ref()
                .is_none_or(|(_, current)| *current != key)
            {
                let context = resampling::Context::get(
                    self.decoded.format(),
                    self.decoded.channel_layout(),
                    self.decoded.rate(),
                    Sample::F32(SampleType::Packed),
                    ChannelLayout::STEREO,
                    self.output_rate,
                )
                .context("failed to configure the audio resampler")?;
                self.resampler = Some((context, key));
            }
            let Some((resampler, _)) = self.resampler.as_mut() else {
                continue;
            };
            let buffered = resampler
                .delay()
                .map_or(0, |delay| delay.output.max(0) as usize);
            let converted = self.decoded.samples() as u64 * u64::from(self.output_rate)
                / u64::from(self.decoded.rate().max(1));
            let capacity = buffered + converted as usize + RESAMPLER_HEADROOM;
            let mut resampled = frame::Audio::new(
                Sample::F32(SampleType::Packed),
                capacity,
                ChannelLayout::STEREO,
            );
            resampler.run(&self.decoded, &mut resampled)?;
            let frames = resampled.samples();
            if frames == 0 {
                continue;
            }
            let pts = self.next_pts();
            self.emitted += frames as u64;
            if self.next_pts() <= self.skip_before {
                continue;
            }
            let bytes = &resampled.data(0)[..frames * AUDIO_CHANNELS * size_of::<f32>()];
            let samples = bytes
                .as_chunks::<{ size_of::<f32>() }>()
                .0
                .iter()
                .map(|raw| f32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]))
                .collect();
            out.push_back(SourceEvent::Audio(AudioChunk { pts, samples }));
        }
        Ok(())
    }
}
