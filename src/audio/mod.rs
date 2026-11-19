pub mod equalizer;
pub mod output;
pub mod params;
pub mod processor;
pub mod renderer;
pub mod silence;
pub mod stretcher;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

pub use equalizer::EqualizerSettings;
pub use params::{AudioControls, MAX_SPEED, MAX_VOLUME, MIN_SPEED};
pub use silence::SilenceSettings;

use crate::engine::{ClockAnchor, SessionShared};
use output::AudioOutput;
use processor::{Processor, SessionSlot};

pub struct AudioEngine {
    controls: Arc<AudioControls>,
    anchor: Arc<ClockAnchor>,
    slot: SessionSlot,
    output: AudioOutput,
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let controls = Arc::new(AudioControls::default());
        let anchor = Arc::new(ClockAnchor::default());
        let slot = SessionSlot::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut writer = None;
        let output = AudioOutput::open(|sample_rate| {
            let (ring, renderer) = renderer::ring(sample_rate, controls.clone(), anchor.clone());
            writer = Some(ring);
            renderer
        });
        let worker = writer.and_then(|ring| {
            let processor =
                Processor::new(controls.clone(), slot.clone(), ring, output.sample_rate);
            let shutdown = shutdown.clone();
            thread::Builder::new()
                .name("veloce-audio-dsp".to_owned())
                .spawn(move || processor.run(shutdown))
                .map_err(|error| log::error!("failed to start the audio thread: {error}"))
                .ok()
        });
        Self {
            controls,
            anchor,
            slot,
            output,
            shutdown,
            worker,
        }
    }

    pub fn controls(&self) -> &Arc<AudioControls> {
        &self.controls
    }

    pub fn anchor(&self) -> Arc<ClockAnchor> {
        self.anchor.clone()
    }

    pub fn sample_rate(&self) -> u32 {
        self.output.sample_rate
    }

    pub fn device_name(&self) -> &str {
        &self.output.device_name
    }

    pub fn attach(&self, session: Option<Arc<SessionShared>>) {
        *self.slot.lock() = session;
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
