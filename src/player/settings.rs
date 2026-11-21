//! Viewer preferences that survive a restart: playback speed, volume and the subtitle choice.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::subtitles::{
    SubtitlePreference, TrackSignature,
    preference::{normalize_language, tag_named},
};

/// Dragging the volume slider changes the settings every frame; writes are spaced this far apart.
const WRITE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq)]
pub struct Preferences {
    pub speed: f32,
    pub volume: f32,
    pub subtitles: SubtitlePreference,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            speed: 1.0,
            volume: 1.0,
            subtitles: SubtitlePreference::Unset,
        }
    }
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Preferences {
    /// One `key value` pair per line, so the file stays readable and editable by hand.
    pub fn to_text(&self) -> String {
        let mut text = format!("speed {:.2}\nvolume {:.2}\n", self.speed, self.volume);
        match &self.subtitles {
            SubtitlePreference::Unset => {}
            SubtitlePreference::Off => text.push_str("subtitles off\n"),
            SubtitlePreference::Track(track) => {
                text.push_str("subtitles track\n");
                if let Some(language) = track.language {
                    text.push_str(&format!("subtitle-language {language}\n"));
                }
                if !track.tags.is_empty() {
                    text.push_str(&format!("subtitle-tags {}\n", track.tags.join(" ")));
                }
                if let Some(title) = &track.title {
                    text.push_str(&format!("subtitle-title {}\n", single_line(title)));
                }
                let source = if track.external { "file" } else { "embedded" };
                text.push_str(&format!("subtitle-source {source}\n"));
                text.push_str(&format!("subtitle-ordinal {}\n", track.ordinal));
            }
        }
        text
    }

    /// Reads what `to_text` wrote; unknown keys and malformed values fall back to the defaults.
    pub fn parse(text: &str) -> Self {
        let mut preferences = Self::default();
        let mut track = TrackSignature {
            language: None,
            tags: Vec::new(),
            title: None,
            external: false,
            ordinal: 0,
        };
        let mut subtitles = None;
        for line in text.lines() {
            let (key, value) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
            let value = value.trim();
            match key {
                "speed" => {
                    if let Ok(speed) = value.parse::<f32>()
                        && speed.is_finite()
                    {
                        preferences.speed = speed;
                    }
                }
                "volume" => {
                    if let Ok(volume) = value.parse::<f32>()
                        && volume.is_finite()
                    {
                        preferences.volume = volume;
                    }
                }
                "subtitles" => subtitles = Some(value == "track"),
                "subtitle-language" => track.language = normalize_language(value),
                "subtitle-tags" => {
                    track.tags = value.split_whitespace().filter_map(tag_named).collect();
                }
                "subtitle-title" if !value.is_empty() => track.title = Some(value.to_lowercase()),
                "subtitle-source" => track.external = value == "file",
                "subtitle-ordinal" => track.ordinal = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        preferences.subtitles = match subtitles {
            None => SubtitlePreference::Unset,
            Some(false) => SubtitlePreference::Off,
            Some(true) => SubtitlePreference::Track(track),
        };
        preferences
    }
}

/// Writes preferences to a file. Without a store path (tests, the default) nothing is written.
#[derive(Default)]
pub struct Settings {
    store: Option<PathBuf>,
    written: String,
    pending: Option<String>,
    last_write: Option<Instant>,
}

impl Settings {
    /// Opens `store`, returning the preferences an earlier run saved there.
    pub fn persistent(store: PathBuf) -> (Self, Preferences) {
        let written = std::fs::read_to_string(&store).unwrap_or_default();
        let preferences = Preferences::parse(&written);
        let settings = Self {
            store: Some(store),
            written,
            pending: None,
            last_write: None,
        };
        (settings, preferences)
    }

    /// Records the current preferences; they reach the disk now or within `WRITE_INTERVAL`.
    pub fn update(&mut self, preferences: &Preferences) {
        if self.store.is_none() {
            return;
        }
        let text = preferences.to_text();
        if text == self.written {
            self.pending = None;
            return;
        }
        self.pending = Some(text);
        self.flush_due();
    }

    /// Writes a pending change once `WRITE_INTERVAL` has passed since the last write.
    pub fn flush_due(&mut self) {
        let due = self
            .last_write
            .is_none_or(|last| last.elapsed() >= WRITE_INTERVAL);
        if due {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        let (Some(store), Some(text)) = (&self.store, self.pending.take()) else {
            return;
        };
        self.last_write = Some(Instant::now());
        let staged = store.with_extension("tmp");
        let written = store
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&staged, &text))
            .and_then(|()| std::fs::rename(&staged, store));
        match written {
            Ok(()) => self.written = text,
            Err(error) => log::warn!("cannot save settings to {}: {error}", store.display()),
        }
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        self.flush();
    }
}
