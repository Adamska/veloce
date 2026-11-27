pub mod command;
mod dispatch;
mod persistence;
pub mod playlist;
mod presenter;
pub mod recent;
pub mod resume;
pub mod session;
pub mod settings;
pub mod subtitles;
mod transport;

use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

pub use command::{Command, SubtitleSelection};
pub use playlist::{DEMO_LOCATION, Playlist, PlaylistItem, RepeatMode};
pub use recent::{RecentEntry, RecentFiles};
pub use resume::{LastSession, Resume};
pub use settings::{Preferences, Settings};
pub use subtitles::SubtitleState;

use crate::{
    audio::{AudioControls, AudioEngine},
    engine::{CacheLimits, CacheSpan, PlaybackClock, TelemetrySnapshot},
    gpu::{Geometry, GpuCapabilities, VideoAdjustments},
    media::{MediaInfo, Micros},
};
use session::Session;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub is_error: bool,
}

pub struct Player {
    audio: AudioEngine,
    clock: PlaybackClock,
    session: Option<Session>,
    capabilities: GpuCapabilities,
    cache_limits: CacheLimits,
    pub playlist: Playlist,
    recent: RecentFiles,
    settings: Settings,
    resume: Resume,
    pub subtitles: SubtitleState,
    pub adjustments: VideoAdjustments,
    pub geometry: Geometry,
    paused: bool,
    ended: bool,
    speed: f32,
    volume: f32,
    muted: bool,
    audio_delay: Micros,
    loop_points: (Option<Micros>, Option<Micros>),
    position: Micros,
    seek_probe: Option<(u64, Instant)>,
    presented: Option<(u64, Micros)>,
    notices: Vec<Notice>,
}

impl Player {
    pub fn new(capabilities: GpuCapabilities, cache_limits: CacheLimits) -> Self {
        let audio = AudioEngine::new();
        let clock = PlaybackClock::new(audio.anchor());
        Self {
            audio,
            clock,
            session: None,
            capabilities,
            cache_limits,
            playlist: Playlist::default(),
            recent: RecentFiles::default(),
            settings: Settings::default(),
            resume: Resume::default(),
            subtitles: SubtitleState::default(),
            adjustments: VideoAdjustments::default(),
            geometry: Geometry::default(),
            paused: true,
            ended: false,
            speed: 1.0,
            volume: 1.0,
            muted: false,
            audio_delay: 0,
            loop_points: (None, None),
            position: 0,
            seek_probe: None,
            presented: None,
            notices: Vec::new(),
        }
    }

    pub fn recent(&self) -> &RecentFiles {
        &self.recent
    }

    pub fn set_recent(&mut self, recent: RecentFiles) {
        self.recent = recent;
    }

    pub fn info(&self) -> Option<&MediaInfo> {
        self.session.as_ref()?.info.as_ref()
    }

    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    pub fn is_loading(&self) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session.info.is_none() || !session.shared.is_ready(session.shared.epoch())
        })
    }

    pub fn position(&self) -> Micros {
        self.position
    }

    pub fn duration(&self) -> Option<Micros> {
        self.info()?.duration
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn audio_delay(&self) -> Micros {
        self.audio_delay
    }

    pub fn loop_points(&self) -> (Option<Micros>, Option<Micros>) {
        self.loop_points
    }

    pub fn audio_track(&self) -> Option<usize> {
        self.session.as_ref()?.shared.audio_track()
    }

    pub fn controls(&self) -> &Arc<AudioControls> {
        self.audio.controls()
    }

    pub fn audio_device(&self) -> (&str, u32) {
        (self.audio.device_name(), self.audio.sample_rate())
    }

    pub fn effective_speed(&self) -> f32 {
        self.controls().effective_speed.load()
    }

    pub fn is_skipping_silence(&self) -> bool {
        self.controls().silence.active.load(Ordering::Relaxed)
    }

    pub fn clock_correction(&self) -> Micros {
        self.clock.last_correction()
    }

    pub fn cache_span(&self) -> Option<CacheSpan> {
        self.session.as_ref()?.shared.cache.span()
    }

    pub fn cache_limits(&self) -> CacheLimits {
        self.cache_limits
    }

    pub fn telemetry(&self) -> Option<TelemetrySnapshot> {
        Some(self.session.as_ref()?.shared.telemetry.snapshot())
    }

    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notices.push(Notice {
            text: text.into(),
            is_error: false,
        });
    }

    /// Shows `text` as long as an error without logging it, for problems already logged.
    pub fn warn(&mut self, text: impl Into<String>) {
        self.notices.push(Notice {
            text: text.into(),
            is_error: true,
        });
    }

    pub fn report_error(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::error!("{text}");
        self.notices.push(Notice {
            text,
            is_error: true,
        });
    }
}
