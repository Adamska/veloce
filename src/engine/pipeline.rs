use std::{sync::Arc, thread, time::Duration};

use anyhow::Result;
use crossbeam_channel::Sender;

use super::{
    session::{SeekRequest, SessionShared},
    telemetry::Telemetry,
};
use crate::media::{MICROS_PER_SECOND, MediaInfo, MediaSource, Micros, RawSubtitle, SourceEvent};

const IDLE_PARK: Duration = Duration::from_millis(4);
const AUDIO_STARVATION_MARGIN: Micros = MICROS_PER_SECOND;
const STARVATION_BUDGET_FACTOR: usize = 2;
const MAX_CONSECUTIVE_ERRORS: u32 = 64;

pub enum PipelineEvent {
    Opened(Box<MediaInfo>),
    Subtitle(RawSubtitle),
    Failed(String),
}

pub type SourceFactory = Box<dyn FnOnce() -> Result<Box<dyn MediaSource>> + Send>;

struct Pipeline {
    source: Box<dyn MediaSource>,
    shared: Arc<SessionShared>,
    events: Sender<PipelineEvent>,
    awaiting: Option<SeekRequest>,
    audio_track: Option<usize>,
    has_audio: bool,
    consecutive_errors: u32,
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
            shared.telemetry.set_backend(source.decoder_backend());
            let has_audio = !info.audio_tracks.is_empty();
            let audio_track = source.selected_audio_track();
            shared.set_audio_track(audio_track);
            let _ = events.send(PipelineEvent::Opened(Box::new(info)));
            let mut pipeline = Pipeline {
                source,
                audio_track,
                shared,
                events,
                awaiting: None,
                has_audio,
                consecutive_errors: 0,
            };
            pipeline.run();
        })
}

impl Pipeline {
    fn run(&mut self) {
        while !self.shared.is_stopped() {
            if let Some(request) = self.shared.take_seek() {
                self.restart_at(request);
                continue;
            }
            let playhead = self.shared.playhead();
            self.shared.cache.trim(playhead);
            self.shared
                .audio
                .trim(playhead, self.shared.cache.limits().behind);
            if self.shared.end_of_stream() || !self.wants_more(playhead) {
                self.shared.park(IDLE_PARK);
                continue;
            }
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
                        let _ = self
                            .events
                            .send(PipelineEvent::Failed(format!("{error:#}")));
                        self.finish();
                    }
                }
            }
        }
    }

    fn restart_at(&mut self, request: SeekRequest) {
        let wanted = self.shared.audio_track().filter(|_| self.has_audio);
        if wanted != self.audio_track {
            match self.source.select_audio_track(wanted) {
                Ok(()) => self.audio_track = wanted,
                Err(error) => log::warn!("audio track switch failed: {error:#}"),
            }
        }
        if let Err(error) = self.source.seek(request.target) {
            log::warn!("seek to {} failed: {error:#}", request.target);
        }
        if self.awaiting.replace(request).is_some() {
            Telemetry::bump(&self.shared.telemetry.seeks_superseded);
        }
    }

    fn wants_more(&self, playhead: Micros) -> bool {
        if !self.shared.has_video() {
            let buffered = self.shared.audio.end_time().unwrap_or(playhead);
            return buffered < playhead + self.shared.cache.limits().ahead;
        }
        if self.shared.cache.wants_more(playhead) {
            return true;
        }
        self.audio_track.is_some()
            && self
                .shared
                .audio
                .end_time()
                .is_none_or(|end| end < playhead + AUDIO_STARVATION_MARGIN)
            && self.shared.cache.span().is_some_and(|span| {
                span.bytes < self.shared.cache.limits().budget_bytes * STARVATION_BUDGET_FACTOR
            })
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
                if self.shared.cache.push(frame) {
                    Telemetry::bump(&self.shared.telemetry.decoded_frames);
                    self.resolve(end);
                }
            }
            SourceEvent::Audio(chunk) => {
                self.shared.audio.push(chunk);
                if !self.shared.has_video() {
                    self.resolve(self.shared.audio.end_time().unwrap_or(Micros::MIN));
                }
            }
            SourceEvent::Subtitle(subtitle) => {
                let _ = self.events.send(PipelineEvent::Subtitle(subtitle));
            }
            SourceEvent::Idle => {}
            SourceEvent::EndOfStream => self.finish(),
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
