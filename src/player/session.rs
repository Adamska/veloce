use std::sync::Arc;

use anyhow::{Context as _, Result};
use crossbeam_channel::Receiver;

use super::playlist::DEMO_LOCATION;
use crate::{
    demo::SyntheticSource,
    engine::{
        CacheLimits, FfmpegSource, OpenOptions, PipelineEvent, SessionShared, SourceFactory,
        demuxer::is_network_location,
        lanes::{self, SPARE_LANES},
        pipeline, speculation,
    },
    gpu::GpuCapabilities,
    media::{MediaInfo, MediaSource, Micros},
};

pub struct Session {
    pub shared: Arc<SessionShared>,
    pub info: Option<MediaInfo>,
    pub location: String,
    events: Receiver<PipelineEvent>,
}

fn source_factory(
    location: &str,
    sample_rate: u32,
    capabilities: GpuCapabilities,
    shared: &Arc<SessionShared>,
    lane: usize,
) -> SourceFactory {
    if location == DEMO_LOCATION {
        return Box::new(move || {
            Ok(Box::new(SyntheticSource::new(sample_rate)) as Box<dyn MediaSource>)
        });
    }
    let location = location.to_owned();
    let cancel = shared.clone();
    let options = OpenOptions {
        sample_rate,
        norm16_textures: capabilities.norm16_textures,
        decode_threads: lanes::decode_threads(lane),
    };
    Box::new(move || {
        let source = FfmpegSource::open(&location, options, move || cancel.is_stopped())?;
        Ok(Box::new(source) as Box<dyn MediaSource>)
    })
}

impl Session {
    /// Starts decoding `location` with the first frame at `start`.
    pub fn open(
        location: &str,
        start: Micros,
        sample_rate: u32,
        capabilities: GpuCapabilities,
        limits: CacheLimits,
    ) -> Result<Self> {
        let spare = if is_network_location(location) {
            0
        } else {
            SPARE_LANES
        };
        let shared = Arc::new(SessionShared::with_spare_lanes(
            limits,
            sample_rate,
            start,
            spare,
        ));
        let (sender, events) = crossbeam_channel::unbounded();
        let factory = |lane| source_factory(location, sample_rate, capabilities, &shared, lane);
        pipeline::spawn(shared.clone(), factory(0), sender.clone())
            .context("failed to start the decode thread")?;
        for lane in 1..=spare {
            speculation::spawn_spare(shared.clone(), lane, factory(lane), sender.clone())
                .context("failed to start a speculative decode thread")?;
        }
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
