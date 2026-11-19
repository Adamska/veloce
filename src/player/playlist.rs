use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::demo::synthetic_source;

pub const DEMO_LOCATION: &str = "veloce://demo";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat all",
            Self::One => "Repeat one",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistItem {
    pub location: String,
    pub title: String,
}

impl PlaylistItem {
    pub fn new(location: String) -> Self {
        let title = if location == DEMO_LOCATION {
            synthetic_source::TITLE.to_owned()
        } else {
            Path::new(location.split('?').next().unwrap_or(&location))
                .file_name()
                .map_or_else(
                    || location.clone(),
                    |name| name.to_string_lossy().into_owned(),
                )
        };
        Self { location, title }
    }
}

#[derive(Debug)]
pub struct Playlist {
    items: Vec<PlaylistItem>,
    current: Option<usize>,
    order: Vec<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    seed: u64,
}

impl Default for Playlist {
    fn default() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x9E37_79B9, |elapsed| elapsed.as_nanos() as u64);
        Self::with_seed(seed)
    }
}

impl Playlist {
    pub fn with_seed(seed: u64) -> Self {
        Self {
            items: Vec::new(),
            current: None,
            order: Vec::new(),
            shuffle: false,
            repeat: RepeatMode::Off,
            seed: seed | 1,
        }
    }

    pub fn items(&self) -> &[PlaylistItem] {
        &self.items
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn current_item(&self) -> Option<&PlaylistItem> {
        self.current.and_then(|index| self.items.get(index))
    }

    fn random(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn reshuffle(&mut self) {
        self.order = (0..self.items.len()).collect();
        for index in (1..self.order.len()).rev() {
            let other = (self.random() % (index as u64 + 1)) as usize;
            self.order.swap(index, other);
        }
        if let Some(current) = self.current
            && let Some(slot) = self.order.iter().position(|index| *index == current)
        {
            self.order.swap(0, slot);
        }
    }

    pub fn replace(&mut self, locations: Vec<String>) {
        self.items = locations.into_iter().map(PlaylistItem::new).collect();
        self.current = None;
        self.reshuffle();
    }

    pub fn extend(&mut self, locations: Vec<String>) {
        self.items
            .extend(locations.into_iter().map(PlaylistItem::new));
        self.reshuffle();
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        self.current = match self.current {
            Some(current) if current == index => None,
            Some(current) if current > index => Some(current - 1),
            other => other,
        };
        self.reshuffle();
    }

    pub fn select(&mut self, index: usize) -> Option<&PlaylistItem> {
        self.current = (index < self.items.len()).then_some(index);
        self.current_item()
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
        self.reshuffle();
    }

    fn neighbor(&self, step: isize) -> Option<usize> {
        let count = self.items.len() as isize;
        if count == 0 {
            return None;
        }
        let sequence: Vec<usize> = if self.shuffle {
            self.order.clone()
        } else {
            (0..self.items.len()).collect()
        };
        let Some(current) = self.current else {
            return sequence.first().copied();
        };
        let slot = sequence.iter().position(|index| *index == current)? as isize + step;
        if (0..count).contains(&slot) {
            sequence.get(slot as usize).copied()
        } else if self.repeat == RepeatMode::All {
            sequence.get(slot.rem_euclid(count) as usize).copied()
        } else {
            None
        }
    }

    pub fn advance(&mut self, automatic: bool) -> Option<usize> {
        let next = if automatic && self.repeat == RepeatMode::One {
            self.current
        } else {
            self.neighbor(1)
        }?;
        self.current = Some(next);
        self.current
    }

    pub fn retreat(&mut self) -> Option<usize> {
        self.current = Some(self.neighbor(-1)?);
        self.current
    }
}
