use std::{sync::Arc, thread, time::Duration};

use anyhow::Result;
use crossbeam_channel::Sender;

use super::{
    session::{SeekRequest, SessionShared},
    telemetry::Telemetry,
};
use crate::media::{MICROS_PER_SECOND, MediaInfo, MediaSource, Micros, RawSubtitle, SourceEvent};

pub(super) const IDLE_PARK: Duration = Duration::from_millis(4);
const AUDIO_STARVATION_MARGIN: Micros = MICROS_PER_SECOND;
const YIELD_AHEAD: Micros = 2 * MICROS_PER_SECOND;
const STARVATION_BUDGET_FACTOR: usize = 2;
const MAX_CONSECUTIVE_ERRORS: u32 = 64;

pub enum PipelineEvent {
    Opened(Box<MediaInfo>),
    Subtitle(RawSubtitle),
    Failed(String),
}

pub type SourceFactory = Box<dyn FnOnce() -> Result<Box<dyn MediaSource>> + Send>;

pub(super) struct Lane {
    pub(super) index: usize,
    pub(super) source: Box<dyn MediaSource>,
    pub(super) shared: Arc<SessionShared>,
    pub(super) events: Sender<PipelineEvent>,
    pub(super) awaiting: Option<SeekRequest>,
    pub(super) audio_track: Option<usize>,
    pub(super) has_audio: bool,
    pub(super) duration: Option<Micros>,
    pub(super) finished: bool,
    consecutive_errors: u32,
}

impl Lane {
    pub(super) fn new(
        index: usize,
        source: Box<dyn MediaSource>,
        shared: Arc<SessionShared>,
        events: Sender<PipelineEvent>,
    ) -> Self {
        Self {
            index,
            audio_track: source.selected_audio_track(),
            has_audio: !source.info().audio_tracks.is_empty(),
            duration: source.info().duration,
            source,
            shared,
            events,
            awaiting: None,
            finished: false,
            consecutive_errors: 0,
        }
    }

    pub(super) fn run(&mut self) {
        while !self.shared.is_stopped() {
            if self.shared.lanes().active() == self.index {
                self.drive_active();
            } else {
                self.awaiting = None;
                self.drive_spare();
            }
        }
    }

    pub(super) fn wanted_audio_track(&self) -> Option<usize> {
        self.shared.audio_track().filter(|_| self.has_audio)
    }

    pub(super) fn reposition(&mut self, target: Micros) {
        let wanted = self.wanted_audio_track();
        if wanted != self.audio_track {
            match self.source.select_audio_track(wanted) {
                Ok(()) => self.audio_track = wanted,
                Err(error) => log::warn!("audio track switch failed: {error:#}"),
            }
        }
        if let Err(error) = self.source.seek(target) {
            log::warn!("seek to {target} failed: {error:#}");
        }
        self.finished = false;
    }

    fn drive_active(&mut self) {
        self.shared.lanes().set_building(self.index, false);
        if let Some(request) = self.shared.take_seek(self.index) {
            let usable = self
                .shared
                .cache
                .span()
                .is_none_or(|span| span.start <= request.target);
            if !request.resumed || !usable || self.wanted_audio_track() != self.audio_track {
                self.shared.cache.flush();
                self.shared.audio.flush();
                self.reposition(request.target);
            }
            if self.awaiting.replace(request).is_some() {
                Telemetry::bump(&self.shared.telemetry.seeks_superseded);
            }
            if self.shared.window_covers(request.target) {
                self.resolve(Micros::MAX);
            }
            return;
        }
        let playhead = self.shared.playhead();
        self.shared.cache.trim(playhead);
        self.shared
            .audio
            .trim(playhead, self.shared.cache.limits().behind);
        if self.finished {
            if !self.shared.end_of_stream() {
                self.finish();
            }
            self.shared.park(IDLE_PARK);
        } else if self.wants_more(playhead) {
            self.pump();
        } else {
            self.shared.park(IDLE_PARK);
        }
    }

    pub(super) fn pump(&mut self) {
        match self.source.next_event() {
            Ok(event) => {
                self.consecutive_errors = 0;
                self.handle(event);
            }
            Err(error) => {
                Telemetry::bump(&self.shared.telemetry.decode_errors);
                self.consecutive_errors += 1;
                log::warn!("decode error: {error:#}");
                if self.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    self.finished = true;
                    if self.shared.lanes().active() == self.index {
                        let _ = self
                            .events
                            .send(PipelineEvent::Failed(format!("{error:#}")));
                        self.finish();
                    }
                }
            }
        }
    }

    fn wants_more(&self, playhead: Micros) -> bool {
        let limits = self.shared.cache.limits();
        if !self.shared.has_video() {
            let buffered = self.shared.audio.end_time().unwrap_or(playhead);
            return buffered < playhead + limits.ahead;
        }
        if self.shared.cache.wants_more(playhead) {
            let comfortable = self
                .shared
                .cache
                .span()
                .is_some_and(|span| span.end >= playhead + YIELD_AHEAD);
            return !(comfortable && self.shared.lanes().any_building());
        }
        self.audio_track.is_some()
            && self
                .shared
                .audio
                .end_time()
                .is_none_or(|end| end < playhead + AUDIO_STARVATION_MARGIN)
            && self
                .shared
                .cache
                .span()
                .is_some_and(|span| span.bytes < limits.budget_bytes * STARVATION_BUDGET_FACTOR)
    }

    fn resolve(&mut self, covered_until: Micros) {
        if let Some(request) = self.awaiting
            && covered_until > request.target
        {
            self.shared
                .telemetry
                .set_backend(self.source.decoder_backend());
            self.shared.mark_ready(request.epoch);
            self.awaiting = None;
        }
    }

    fn handle(&mut self, event: SourceEvent) {
        match event {
            SourceEvent::Video(frame) => {
                let end = frame.end();
                if self.shared.deliver_video(self.index, frame) == Some(true) {
                    Telemetry::bump(&self.shared.telemetry.decoded_frames);
                    self.resolve(end);
                }
            }
            SourceEvent::Audio(chunk) => {
                if self.shared.deliver_audio(self.index, chunk) && !self.shared.has_video() {
                    self.resolve(self.shared.audio.end_time().unwrap_or(Micros::MIN));
                }
            }
            SourceEvent::Subtitle(subtitle) => {
                let _ = self.events.send(PipelineEvent::Subtitle(subtitle));
            }
            SourceEvent::Idle => {}
            SourceEvent::EndOfStream => {
                self.finished = true;
                if self.shared.lanes().active() == self.index {
                    self.finish();
                }
            }
        }
    }

    fn finish(&mut self) {
        let video_end = self.shared.cache.span().map_or(0, |span| span.end);
        let audio_end = self.shared.audio.end_time().unwrap_or(0);
        self.shared.set_end_of_stream(video_end.max(audio_end));
        if let Some(request) = self.awaiting.take() {
            self.shared.mark_ready(request.epoch);
        }
    }
}

pub fn spawn(
    shared: Arc<SessionShared>,
    factory: SourceFactory,
    events: Sender<PipelineEvent>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("veloce-decode".to_owned())
        .spawn(move || {
            let source = match factory() {
                Ok(source) => source,
                Err(error) => {
                    let _ = events.send(PipelineEvent::Failed(format!("{error:#}")));
                    return;
                }
            };
            let info = source.info().clone();
            shared.set_has_video(info.video.is_some());
            shared.set_audio_track(source.selected_audio_track());
            shared.telemetry.set_backend(source.decoder_backend());
            let _ = events.send(PipelineEvent::Opened(Box::new(info)));
            Lane::new(0, source, shared, events).run();
        })
}
