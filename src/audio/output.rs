use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_channel::Receiver;

use super::renderer::Renderer;

const FALLBACK_RATE: u32 = 48_000;
const NULL_PERIOD: Duration = Duration::from_millis(10);

pub struct AudioOutput {
    pub sample_rate: u32,
    pub device_name: String,
    _stream: Option<cpal::Stream>,
    null_stop: Option<Arc<AtomicBool>>,
}

struct Device {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    name: String,
}

fn default_device() -> Result<Device> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no audio output device is available")?;
    let config = device
        .default_output_config()
        .context("the output device reports no default configuration")?;
    let name = device.description().map_or_else(
        |_| "Default output".to_owned(),
        |info| info.name().to_owned(),
    );
    Ok(Device {
        device,
        config,
        name,
    })
}

fn build<T>(device: &Device, handoff: Receiver<Renderer>) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let config = device.config.config();
    let channels = usize::from(config.channels);
    let mut scratch = vec![0.0f32; 16_384 * channels];
    let mut renderer: Option<Renderer> = None;
    let stream = device.device.build_output_stream(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            if renderer.is_none() {
                renderer = handoff.try_recv().ok();
            }
            if scratch.len() < data.len() {
                scratch.resize(data.len(), 0.0);
            }
            let block = &mut scratch[..data.len()];
            match renderer.as_mut() {
                Some(renderer) => {
                    let timestamp = info.timestamp();
                    let latency = timestamp.playback.duration_since(timestamp.callback);
                    renderer.render(block, channels, latency.as_nanos() as u64);
                }
                None => block.fill(0.0),
            }
            for (target, sample) in data.iter_mut().zip(block.iter()) {
                *target = T::from_sample(*sample);
            }
        },
        |error| log::warn!("audio stream error: {error}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

fn open_stream(device: &Device, handoff: Receiver<Renderer>) -> Result<cpal::Stream> {
    match device.config.sample_format() {
        SampleFormat::F32 => build::<f32>(device, handoff),
        SampleFormat::I16 => build::<i16>(device, handoff),
        SampleFormat::U16 => build::<u16>(device, handoff),
        SampleFormat::I32 => build::<i32>(device, handoff),
        other => bail!("unsupported output sample format {other}"),
    }
}

fn spawn_null_device(mut renderer: Renderer, stop: Arc<AtomicBool>) {
    let frames = (renderer.sample_rate() / 100) as usize;
    let spawned = thread::Builder::new()
        .name("veloce-null-audio".to_owned())
        .spawn(move || {
            let mut block = vec![0.0f32; frames * 2];
            let mut deadline = Instant::now();
            while !stop.load(Ordering::Acquire) {
                renderer.render(&mut block, 2, 0);
                deadline += NULL_PERIOD;
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
            }
        });
    if let Err(error) = spawned {
        log::error!("failed to start the silent audio clock: {error}");
    }
}

impl AudioOutput {
    pub fn open(make_renderer: impl FnOnce(u32) -> Renderer) -> Self {
        let device = default_device();
        let sample_rate = device
            .as_ref()
            .map_or(FALLBACK_RATE, |device| device.config.sample_rate());
        let renderer = make_renderer(sample_rate);
        let opened = device.and_then(|device| {
            let (sender, handoff) = crossbeam_channel::bounded(1);
            open_stream(&device, handoff).map(|stream| (device.name, stream, sender))
        });
        match opened {
            Ok((device_name, stream, sender)) => {
                let _ = sender.send(renderer);
                Self {
                    sample_rate,
                    device_name,
                    _stream: Some(stream),
                    null_stop: None,
                }
            }
            Err(error) => {
                log::warn!("audio output unavailable, playback will be silent: {error:#}");
                Self::silent(sample_rate, renderer)
            }
        }
    }

    fn silent(sample_rate: u32, renderer: Renderer) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        spawn_null_device(renderer, stop.clone());
        Self {
            sample_rate,
            device_name: "No audio device".to_owned(),
            _stream: None,
            null_stop: Some(stop),
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        if let Some(stop) = &self.null_stop {
            stop.store(true, Ordering::Release);
        }
    }
}
