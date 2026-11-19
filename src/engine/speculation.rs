use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::Sender;

use super::{
    lanes::ROLL_LIMIT,
    pipeline::{IDLE_PARK, Lane, PipelineEvent, SourceFactory},
    session::SessionShared,
};
use crate::media::Micros;

const DORMANT_PARK: Duration = Duration::from_millis(25);
const YIELD_PARK: Duration = Duration::from_millis(1);
const END_MARGIN: Micros = 100_000;

impl Lane {
    fn anchor(&self) -> Option<Micros> {
        let lanes = self.shared.lanes();
        let offset = lanes.offset(self.index).filter(|_| lanes.is_warm())?;
        let anchor = self.shared.playhead() + offset;
        let inside = anchor >= 0 && self.duration.is_none_or(|end| anchor < end - END_MARGIN);
        let redundant =
            self.shared.window_covers(anchor) && self.shared.window_covers(anchor + ROLL_LIMIT);
        (inside && !redundant).then_some(anchor)
    }

    pub(super) fn drive_spare(&mut self) {
        let shared = self.shared.clone();
        let lanes = shared.lanes();
        let pad = lanes.pad(self.index);
        let video = shared.has_video();
        let Some(anchor) = self.anchor() else {
            lanes.set_building(self.index, false);
            if !pad.is_empty() && shared.begin_retarget(self.index) {
                pad.set_aim(None);
            }
            shared.nap(DORMANT_PARK);
            return;
        };
        if !lanes.serves_next_hop(self.index) && !shared.is_ready(shared.epoch()) {
            lanes.set_building(self.index, false);
            shared.nap(YIELD_PARK);
            return;
        }
        let stale = pad.take_stale() || self.wanted_audio_track() != self.audio_track;
        if stale || !pad.serves(anchor, video) {
            if !shared.begin_retarget(self.index) {
                return;
            }
            pad.set_aim(Some(anchor));
            self.reposition(anchor);
        }
        if pad.covers(anchor, video) {
            pad.set_aim(None);
        }
        pad.trim(anchor);
        let building = !self.finished && pad.wants_more(anchor, video);
        lanes.set_building(self.index, building);
        if building {
            self.pump();
        } else {
            shared.nap(IDLE_PARK);
        }
    }
}

pub fn spawn_spare(
    shared: Arc<SessionShared>,
    lane: usize,
    factory: SourceFactory,
    events: Sender<PipelineEvent>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name(format!("veloce-lane-{lane}"))
        .spawn(move || {
            while !shared.has_started() {
                if shared.is_stopped() {
                    return;
                }
                shared.nap(DORMANT_PARK);
            }
            match factory() {
                Ok(source) => Lane::new(lane, source, shared, events).run(),
                Err(error) => log::warn!("speculative decoder {lane} unavailable: {error:#}"),
            }
        })
}
