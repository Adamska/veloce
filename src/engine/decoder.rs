use std::collections::VecDeque;

use anyhow::{Context as _, Result};
use ffmpeg_next::{
    self as ffmpeg, Rational, Rescale, codec, decoder, format::Pixel, frame, rescale, threading,
};

use super::{
    hwaccel::{self, HardwareDevice},
    video_convert::FrameConverter,
};
use crate::media::{Micros, SourceEvent};

const FALLBACK_THRESHOLD: u32 = 6;

pub struct VideoDecoder {
    decoder: decoder::Video,
    device: Option<HardwareDevice>,
    parameters: codec::Parameters,
    converter: FrameConverter,
    time_base: Rational,
    start_offset: Micros,
    nominal_duration: Micros,
    skip_before: Micros,
    decoded: frame::Video,
    hardware_active: bool,
    produced: u64,
    failures: u32,
}

fn build(
    parameters: &codec::Parameters,
    hardware: bool,
) -> Result<(decoder::Video, Option<HardwareDevice>)> {
    let mut context = codec::Context::from_parameters(parameters.clone())
        .context("unsupported video codec parameters")?;
    let device = hardware
        .then(|| decoder::find(context.id()))
        .flatten()
        .and_then(|codec| HardwareDevice::open_for(&codec));
    match &device {
        Some(device) => device.attach(&mut context),
        None => context.set_threading(threading::Config {
            kind: threading::Type::Frame,
            count: 0,
        }),
    }
    let decoder = context
        .decoder()
        .video()
        .context("failed to open the video decoder")?;
    Ok((decoder, device))
}

impl VideoDecoder {
    pub fn open(
        stream: &ffmpeg::Stream,
        start_offset: Micros,
        frame_rate: f64,
        norm16: bool,
    ) -> Result<Self> {
        let parameters = stream.parameters();
        let (decoder, device) = match build(&parameters, true) {
            Ok(built) => built,
            Err(error) => {
                log::warn!("hardware decoder setup failed, using software: {error:#}");
                build(&parameters, false)?
            }
        };
        let nominal_duration = if frame_rate > 0.0 {
            (1e6 / frame_rate).round() as Micros
        } else {
            40_000
        };
        Ok(Self {
            hardware_active: device.is_some(),
            decoder,
            device,
            parameters,
            converter: FrameConverter::new(norm16),
            time_base: stream.time_base(),
            start_offset,
            nominal_duration,
            skip_before: Micros::MIN,
            decoded: frame::Video::empty(),
            produced: 0,
            failures: 0,
        })
    }

    pub fn backend(&self) -> String {
        match &self.device {
            Some(device) if self.hardware_active => format!("{} (hardware)", device.name()),
            _ => "FFmpeg software (frame threads)".to_owned(),
        }
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    pub fn skip_before(&mut self, threshold: Micros) {
        self.skip_before = threshold;
    }

    fn fall_back_to_software(&mut self) -> Result<()> {
        log::warn!("hardware decoding failed repeatedly, switching to software decoding");
        let (decoder, _) = build(&self.parameters, false)?;
        self.decoder = decoder;
        self.device = None;
        self.hardware_active = false;
        self.failures = 0;
        Ok(())
    }

    pub fn send(
        &mut self,
        packet: &ffmpeg::Packet,
        out: &mut VecDeque<SourceEvent>,
    ) -> Result<bool> {
        if let Err(error) = self.decoder.send_packet(packet) {
            self.failures += 1;
            if self.device.is_some() && self.produced == 0 && self.failures >= FALLBACK_THRESHOLD {
                self.fall_back_to_software()?;
                return Ok(true);
            }
            if !matches!(error, ffmpeg::Error::InvalidData) {
                return Err(error).context("video packet rejected");
            }
        }
        self.receive(out)?;
        Ok(false)
    }

    pub fn drain(&mut self, out: &mut VecDeque<SourceEvent>) -> Result<()> {
        let _ = self.decoder.send_eof();
        self.receive(out)
    }

    fn receive(&mut self, out: &mut VecDeque<SourceEvent>) -> Result<()> {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            let timestamp = self.decoded.timestamp().or(self.decoded.pts()).unwrap_or(0);
            let pts = timestamp.rescale(self.time_base, rescale::TIME_BASE) - self.start_offset;
            let duration = match self.decoded.duration() {
                value if value > 0 => value.rescale(self.time_base, rescale::TIME_BASE),
                _ => self.nominal_duration,
            };
            if pts + duration <= self.skip_before {
                continue;
            }
            let on_surface = self
                .device
                .as_ref()
                .is_some_and(|device| device.produces(&self.decoded));
            self.hardware_active = on_surface;
            let picture = if on_surface {
                hwaccel::download(&self.decoded)?
            } else {
                std::mem::replace(&mut self.decoded, frame::Video::empty())
            };
            if picture.format() == Pixel::None {
                continue;
            }
            let frame = self.converter.convert(picture, pts, duration)?;
            self.produced += 1;
            self.failures = 0;
            out.push_back(SourceEvent::Video(frame));
        }
        Ok(())
    }
}
