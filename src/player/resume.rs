//! Where playback stopped in each file, and the queue that was open when Veloce last ran, so a
//! relaunch picks up at the same frame.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use super::recent::now_seconds;
use crate::media::{MICROS_PER_SECOND, Micros};

const MAX_POSITIONS: usize = 200;
const WRITE_INTERVAL: Duration = Duration::from_secs(5);
/// Closer to either end than this, a file starts from the beginning next time.
pub const MIN_RESUME: Micros = 5 * MICROS_PER_SECOND;
pub const END_MARGIN: Micros = 10 * MICROS_PER_SECOND;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub location: String,
    pub position: Micros,
    /// Seconds since the Unix epoch.
    pub saved: u64,
}

/// The queue that was open when Veloce quit or crashed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastSession {
    pub locations: Vec<String>,
    pub current: usize,
    pub paused: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResumeState {
    pub session: Option<LastSession>,
    /// Newest first.
    pub bookmarks: Vec<Bookmark>,
}

fn storable(location: &str) -> bool {
    !location.is_empty() && !location.contains(['\n', '\t'])
}

impl ResumeState {
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        if let Some(session) = &self.session {
            let state = if session.paused { "paused" } else { "playing" };
            text.push_str(&format!("session\t{}\t{state}\n", session.current));
            for location in &session.locations {
                text.push_str(&format!("queue\t{location}\n"));
            }
        }
        for bookmark in &self.bookmarks {
            let (position, saved) = (bookmark.position, bookmark.saved);
            text.push_str(&format!(
                "position\t{position}\t{saved}\t{}\n",
                bookmark.location
            ));
        }
        text
    }

    /// Reads what `to_text` wrote, skipping damaged lines.
    pub fn parse(text: &str) -> Self {
        let mut state = Self::default();
        let mut session: Option<LastSession> = None;
        for line in text.lines() {
            let mut fields = line.splitn(4, '\t');
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some("session"), Some(current), Some(paused), None) => {
                    if let Ok(current) = current.parse() {
                        session = Some(LastSession {
                            locations: Vec::new(),
                            current,
                            paused: paused == "paused",
                        });
                    }
                }
                (Some("queue"), Some(location), None, None) => {
                    if let Some(session) = session.as_mut().filter(|_| storable(location)) {
                        session.locations.push(location.to_owned());
                    }
                }
                (Some("position"), Some(position), Some(saved), Some(location)) => {
                    if let (Ok(position), Ok(saved)) = (position.parse(), saved.parse())
                        && storable(location)
                        && state.bookmarks.len() < MAX_POSITIONS
                    {
                        state.bookmarks.push(Bookmark {
                            location: location.to_owned(),
                            position,
                            saved,
                        });
                    }
                }
                _ => {}
            }
        }
        state.session = session.filter(|session| session.current < session.locations.len());
        state
    }
}

/// Persists a [`ResumeState`]; in memory only unless given a store path. The player flushes it
/// every few seconds while running, when the app exits and on drop.
#[derive(Default)]
pub struct Resume {
    state: ResumeState,
    store: Option<PathBuf>,
    written: String,
    last_flush: Option<Instant>,
}

impl Resume {
    pub fn persistent(store: PathBuf) -> Self {
        let written = std::fs::read_to_string(&store).unwrap_or_default();
        Self {
            state: ResumeState::parse(&written),
            store: Some(store),
            written,
            last_flush: None,
        }
    }

    pub fn last_session(&self) -> Option<&LastSession> {
        self.state.session.as_ref()
    }

    pub fn position(&self, location: &str) -> Option<Micros> {
        self.state
            .bookmarks
            .iter()
            .find(|bookmark| bookmark.location == location)
            .map(|bookmark| bookmark.position)
    }

    /// Remembers `position` in `location`, or forgets it when it is too close to either end
    /// of `duration` to be worth resuming.
    pub fn remember(&mut self, location: &str, position: Micros, duration: Option<Micros>) {
        self.state
            .bookmarks
            .retain(|bookmark| bookmark.location != location);
        let near_end = duration.is_some_and(|duration| position > duration - END_MARGIN);
        if position >= MIN_RESUME && !near_end && storable(location) {
            self.state.bookmarks.insert(
                0,
                Bookmark {
                    location: location.to_owned(),
                    position,
                    saved: now_seconds(),
                },
            );
            self.state.bookmarks.truncate(MAX_POSITIONS);
        }
    }

    pub fn set_session(&mut self, session: Option<LastSession>) {
        self.state.session = session.filter(|session| {
            session.current < session.locations.len()
                && session.locations.iter().all(|location| storable(location))
        });
    }

    pub fn flush_due(&self) -> bool {
        self.last_flush
            .is_none_or(|flushed| flushed.elapsed() >= WRITE_INTERVAL)
    }

    pub fn flush(&mut self) {
        self.last_flush = Some(Instant::now());
        let Some(store) = &self.store else {
            return;
        };
        let text = self.state.to_text();
        if text == self.written {
            return;
        }
        let temporary = store.with_extension("tmp");
        let written = store
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&temporary, &text))
            .and_then(|()| std::fs::rename(&temporary, store));
        match written {
            Ok(()) => self.written = text,
            Err(error) => log::warn!(
                "cannot save playback positions to {}: {error}",
                store.display()
            ),
        }
    }
}

impl Drop for Resume {
    fn drop(&mut self) {
        self.flush();
    }
}
