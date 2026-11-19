use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;

const MAX_ENTRIES: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// Seconds since the Unix epoch.
    pub opened: u64,
}

#[derive(Default)]
struct Availability {
    checked: bool,
    missing: HashSet<PathBuf>,
}

/// Files played before, newest first. Only persisted when given a store path.
pub struct RecentFiles {
    entries: Vec<RecentEntry>,
    store: Option<PathBuf>,
    availability: Arc<Mutex<Availability>>,
}

impl Default for RecentFiles {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            store: None,
            availability: Arc::new(Mutex::new(Availability {
                checked: true,
                missing: HashSet::new(),
            })),
        }
    }
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn parse(text: &str) -> Vec<RecentEntry> {
    text.lines()
        .filter_map(|line| {
            let (opened, path) = line.split_once('\t')?;
            Some(RecentEntry {
                path: PathBuf::from(path),
                opened: opened.parse().ok()?,
            })
        })
        .take(MAX_ENTRIES)
        .collect()
}

impl RecentFiles {
    /// Loads `store` and checks on a background thread which files still exist, so an
    /// unreachable network volume never blocks the interface. `checked` runs afterwards.
    pub fn persistent(store: PathBuf, checked: impl FnOnce() + Send + 'static) -> Self {
        let entries = std::fs::read_to_string(&store)
            .map(|text| parse(&text))
            .unwrap_or_default();
        let availability = Arc::new(Mutex::new(Availability::default()));
        let paths: Vec<PathBuf> = entries.iter().map(|entry| entry.path.clone()).collect();
        let shared = availability.clone();
        let spawned = std::thread::Builder::new()
            .name("recent-files".into())
            .spawn(move || {
                let missing = paths.into_iter().filter(|path| !path.is_file()).collect();
                *shared.lock() = Availability {
                    checked: true,
                    missing,
                };
                checked();
            });
        if let Err(error) = spawned {
            log::warn!("cannot check recent files: {error}");
            availability.lock().checked = true;
        }
        Self {
            entries,
            store: Some(store),
            availability,
        }
    }

    /// Recent files that still exist, newest first.
    pub fn available(&self) -> Vec<RecentEntry> {
        let availability = self.availability.lock();
        if !availability.checked {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|entry| !availability.missing.contains(&entry.path))
            .cloned()
            .collect()
    }

    pub fn record(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if path.to_string_lossy().contains(['\n', '\t']) {
            return;
        }
        self.availability.lock().missing.remove(&path);
        self.entries.retain(|entry| entry.path != path);
        self.entries.insert(
            0,
            RecentEntry {
                path,
                opened: now_seconds(),
            },
        );
        self.entries.truncate(MAX_ENTRIES);
        self.save();
    }

    pub fn forget(&mut self, path: &Path) {
        self.entries.retain(|entry| entry.path != path);
        self.save();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    fn save(&self) {
        let Some(store) = &self.store else {
            return;
        };
        let text: String = self
            .entries
            .iter()
            .map(|entry| format!("{}\t{}\n", entry.opened, entry.path.display()))
            .collect();
        let written = store
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(store, text));
        if let Err(error) = written {
            log::warn!("cannot save recent files to {}: {error}", store.display());
        }
    }
}
