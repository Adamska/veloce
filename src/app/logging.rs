//! The log file. Every thread's records go to stderr and to `veloce.log`, panics are written
//! with a backtrace, and a locked marker file per run lets the next launch tell whether an
//! earlier run ended without a clean exit (crash, force quit or power loss).

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::files;

pub const FILE_NAME: &str = "veloce.log";
const PREVIOUS_NAME: &str = "veloce.old.log";
const MARKER_PREFIX: &str = "running-";
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const DEFAULT_FILTER: &str = "info,wgpu_core=warn,wgpu_hal=warn,naga=warn";

/// Where the log of this run is written.
pub fn log_path() -> Option<PathBuf> {
    files::log_dir().map(|directory| directory.join(FILE_NAME))
}

/// This run's place in the log directory. Call [`LogSession::finish`] on a clean exit.
#[derive(Default)]
pub struct LogSession {
    marker: Option<(PathBuf, File)>,
    /// Process ids of earlier runs that never reached a clean exit.
    pub unclean_exits: Vec<u32>,
}

/// Duplicates records to stderr and to the log file. A failed file write stops file logging
/// rather than the app.
struct Tee {
    file: Option<File>,
}

impl Write for Tee {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buffer);
        if let Some(file) = self.file.as_mut()
            && file.write_all(buffer).is_err()
        {
            self.file = None;
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

/// Markers of earlier runs whose process no longer holds the lock: those runs crashed or were
/// killed. Markers of runs still alive are locked and left alone.
fn collect_unclean_exits(directory: &Path) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.strip_prefix(MARKER_PREFIX))
            .and_then(|pid| pid.parse().ok())
        else {
            continue;
        };
        let abandoned = File::open(entry.path()).is_ok_and(|marker| marker.try_lock().is_ok());
        if abandoned {
            pids.push(pid);
            let _ = fs::remove_file(entry.path());
        }
    }
    pids.sort_unstable();
    pids
}

/// Rotates an oversized log, records earlier unclean exits and claims this run's marker.
pub fn prepare(directory: &Path) -> io::Result<(File, LogSession)> {
    fs::create_dir_all(directory)?;
    let path = directory.join(FILE_NAME);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_BYTES) {
        fs::rename(&path, directory.join(PREVIOUS_NAME))?;
    }
    let unclean_exits = collect_unclean_exits(directory);
    let marker_path = directory.join(format!("{MARKER_PREFIX}{}", std::process::id()));
    let marker = File::create(&marker_path)?;
    marker.lock()?;
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let session = LogSession {
        marker: Some((marker_path, marker)),
        unclean_exits,
    };
    Ok((file, session))
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>");
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("thread '{name}' {info}\nbacktrace:\n{backtrace}");
    }));
}

/// Installs the logger and the panic hook. Logs to stderr only when `directory` is missing or
/// cannot be written.
pub fn init(directory: Option<PathBuf>) -> LogSession {
    let prepared = directory.as_deref().map(prepare);
    let (file, session, failure) = match prepared {
        Some(Ok((file, session))) => (Some(file), session, None),
        Some(Err(error)) => (None, LogSession::default(), Some(error)),
        None => (None, LogSession::default(), None),
    };
    let _ =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_FILTER))
            .format_timestamp_millis()
            .target(env_logger::Target::Pipe(Box::new(Tee { file })))
            .try_init();
    install_panic_hook();
    log::info!(
        "Veloce {} starting on {} {} (pid {}), times are UTC",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id()
    );
    if let (Some(error), Some(directory)) = (failure, directory) {
        log::warn!(
            "cannot write the log file in {}: {error}",
            directory.display()
        );
    }
    for pid in &session.unclean_exits {
        log::warn!("the run with pid {pid} did not exit cleanly (crash, force quit or power loss)");
    }
    if !session.unclean_exits.is_empty() && cfg!(target_os = "macos") {
        log::warn!("crash reports from macOS, if any, are in ~/Library/Logs/DiagnosticReports");
    }
    session
}

impl LogSession {
    pub fn crashed_before(&self) -> bool {
        !self.unclean_exits.is_empty()
    }

    /// Records a clean exit; the next launch will not report this run.
    pub fn finish(&mut self) {
        if let Some((path, marker)) = self.marker.take() {
            log::info!("exiting cleanly");
            drop(marker);
            let _ = fs::remove_file(path);
        }
    }
}
