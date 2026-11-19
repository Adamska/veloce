use std::collections::VecDeque;

use anyhow::{Context as _, Result};
use ffmpeg_next::{
    self as ffmpeg, Discard, Rational, Rescale, codec, decoder, ffi, format::Pixel, frame, rescale,
    threading,
};

use super::{
    hwaccel::{self, HardwareDevice, Preference},
    video_convert::FrameConverter,
};
use crate::media::{Micros, SourceEvent};

const FALLBACK_THRESHOLD: u32 = 6;
const SOFTWARE_PIXEL_LIMIT: i64 = 2560 * 1440;
const SOFTWARE_MIN_CORES: usize = 6;

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
    threads: usize,
    produced: u64,
    failures: u32,
}

fn software_is_faster(parameters: &codec::Parameters) -> bool {
    // SAFETY: the parameters pointer is owned by `parameters` for the duration of the borrow.
    let (width, height, format) = unsafe {
        let raw = &*parameters.as_ptr();
        (raw.width, raw.height, raw.format)
    };
    // SAFETY: the descriptor table is static and a null result is handled.
    let depth = unsafe {
        ffi::av_pix_fmt_desc_get(std::mem::transmute::<i32, ffi::AVPixelFormat>(format))
            .as_ref()
            .map_or(8, |descriptor| descriptor.comp[0].depth)
    };
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let light_codec = matches!(
        parameters.id(),
        codec::Id::H264
            | codec::Id::MPEG2VIDEO
            | codec::Id::MPEG4
            | codec::Id::VC1
            | codec::Id::VP8
            | codec::Id::VP9
    );
    light_codec
        && depth <= 8
        && i64::from(width) * i64::from(height) <= SOFTWARE_PIXEL_LIMIT
        && cores >= SOFTWARE_MIN_CORES
}

fn build(
    parameters: &codec::Parameters,
    hardware: bool,
    threads: usize,
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
            count: threads,
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
        threads: usize,
    ) -> Result<Self> {
        let parameters = stream.parameters();
        let hardware = match hwaccel::preference() {
            Preference::Hardware => true,
            Preference::Software => false,
            Preference::Automatic => !software_is_faster(&parameters),
        };
        let (decoder, device) = match build(&parameters, hardware, threads) {
            Ok(built) => built,
            Err(error) => {
                log::warn!("hardware decoder setup failed, using software: {error:#}");
                build(&parameters, false, threads)?
            }
        };
        let nominal_duration = if frame_rate > 0.0 {
            (1e6 / frame_rate).round() as Micros
        } else {
            40_000
        };
        Ok(Self {
            hardware_active: device.is_some(),
            threads,
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
        let (decoder, _) = build(&self.parameters, false, self.threads)?;
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
        let skippable = packet.pts().or(packet.dts()).is_some_and(|timestamp| {
            let pts = timestamp.rescale(self.time_base, rescale::TIME_BASE) - self.start_offset;
            pts + self.nominal_duration <= self.skip_before
        });
        self.decoder.skip_frame(if skippable {
            Discard::NonReference
        } else {
            Discard::Default
        });
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
