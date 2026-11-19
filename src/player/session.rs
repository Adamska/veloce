use std::sync::Arc;

use anyhow::{Context as _, Result};
use crossbeam_channel::Receiver;

use super::playlist::DEMO_LOCATION;
use crate::{
    demo::SyntheticSource,
    engine::{
        CacheLimits, FfmpegSource, OpenOptions, PipelineEvent, SessionShared, SourceFactory,
        pipeline,
    },
    gpu::GpuCapabilities,
    media::{MediaInfo, MediaSource},
};

pub struct Session {
    pub shared: Arc<SessionShared>,
    pub info: Option<MediaInfo>,
    pub location: String,
    events: Receiver<PipelineEvent>,
}

impl Session {
    pub fn open(
        location: &str,
        sample_rate: u32,
        capabilities: GpuCapabilities,
        limits: CacheLimits,
    ) -> Result<Self> {
        let shared = Arc::new(SessionShared::new(limits, sample_rate, 0));
        let (sender, events) = crossbeam_channel::unbounded();
        let factory: SourceFactory = if location == DEMO_LOCATION {
            Box::new(
                move || Ok(Box::new(SyntheticSource::new(sample_rate)) as Box<dyn MediaSource>),
            )
        } else {
            let location = location.to_owned();
            let cancel = shared.clone();
            let options = OpenOptions {
                sample_rate,
                norm16_textures: capabilities.norm16_textures,
            };
            Box::new(move || {
                let source = FfmpegSource::open(&location, options, move || cancel.is_stopped())?;
                Ok(Box::new(source) as Box<dyn MediaSource>)
            })
        };
        pipeline::spawn(shared.clone(), factory, sender)
            .context("failed to start the decode thread")?;
        Ok(Self {
            shared,
            info: None,
            location: location.to_owned(),
            events,
        })
    }

    pub fn drain_events(&self) -> Vec<PipelineEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.stop();
    }
}
