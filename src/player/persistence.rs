//! What the player keeps across launches: preferences, where each file stopped and the queue
//! that was open.

use super::{DEMO_LOCATION, LastSession, Player, Preferences, Resume, Settings, command};
use crate::{
    audio::{MAX_SPEED, MAX_VOLUME, MIN_SPEED},
    media::Micros,
};

impl Player {
    /// Switches to persisted settings and applies what an earlier run saved.
    pub fn set_settings(&mut self, settings: Settings, saved: Preferences) {
        self.settings = settings;
        self.speed = saved.speed.clamp(MIN_SPEED, MAX_SPEED);
        self.controls().set_speed(self.speed);
        self.volume = saved.volume.clamp(0.0, MAX_VOLUME);
        self.controls().set_volume(self.volume);
        self.subtitles.restore(saved.subtitles);
    }

    pub fn preferences(&self) -> Preferences {
        Preferences {
            speed: self.speed,
            volume: self.volume,
            subtitles: self.subtitles.preference(),
        }
    }

    pub(super) fn track_settings(&mut self) {
        let preferences = self.preferences();
        self.settings.update(&preferences);
    }

    /// Switches to persisted playback positions.
    pub fn set_resume(&mut self, resume: Resume) {
        self.resume = resume;
    }

    /// Reopens the queue of the previous run at the frame where it stopped, paused if it was.
    /// Returns false when there was nothing to restore.
    pub fn restore_session(&mut self) -> bool {
        let Some(last) = self.resume.last_session().cloned() else {
            return false;
        };
        let location = &last.locations[last.current];
        log::info!("restoring the previous session at {location}");
        self.playlist.replace(last.locations);
        self.start(last.current);
        if last.paused && self.session.is_some() {
            self.set_paused(true);
        }
        self.session.is_some()
    }

    /// Writes settings, playback positions and the open queue at once, for use on exit.
    pub fn save_state(&mut self) {
        self.track_settings();
        self.settings.flush();
        self.record_progress();
        self.resume.flush();
    }

    /// Runs every frame; saves the position every few seconds so a crash loses little.
    pub(super) fn checkpoint(&mut self) {
        self.settings.flush_due();
        if self.resume.flush_due() {
            self.record_progress();
            self.resume.flush();
        }
    }

    /// Where `location` should start: the saved position, or the beginning.
    pub(super) fn resume_point(&self, location: &str) -> Micros {
        self.resume.position(location).unwrap_or(0)
    }

    pub(super) fn announce_resume(&mut self, position: Micros) {
        log::info!("resuming at {}", command::format_clock(position));
        self.notify(format!("Resumed at {}", command::format_clock(position)));
    }

    /// Saves where the open file stands, before it is closed.
    pub(super) fn remember_position(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(info) = session.info.as_ref() else {
            return;
        };
        if info.seekable && session.location != DEMO_LOCATION {
            let (location, duration) = (session.location.clone(), info.duration);
            self.resume.remember(&location, self.position, duration);
        }
    }

    fn record_progress(&mut self) {
        self.remember_position();
        let queue = self.open_queue();
        self.resume.set_session(queue);
    }

    fn open_queue(&self) -> Option<LastSession> {
        self.session.as_ref()?;
        let items = self.playlist.items();
        if items.iter().any(|item| item.location == DEMO_LOCATION) {
            return None;
        }
        Some(LastSession {
            locations: items.iter().map(|item| item.location.clone()).collect(),
            current: self.playlist.current()?,
            paused: self.paused || self.ended,
        })
    }
}
