use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

#[derive(Default)]
pub struct Telemetry {
    pub seeks_hit: AtomicU64,
    pub seeks_missed: AtomicU64,
    pub seeks_coalesced: AtomicU64,
    pub seeks_superseded: AtomicU64,
    pub seeks_prepared: AtomicU64,
    pub decode_flushes: AtomicU64,
    pub last_resolve_nanos: AtomicU64,
    pub last_present_micros: AtomicU64,
    pub decoded_frames: AtomicU64,
    pub dropped_frames: AtomicU64,
    pub decode_errors: AtomicU64,
    pub silence_skipped_micros: AtomicU64,
    backend: Mutex<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetrySnapshot {
    pub seeks_hit: u64,
    pub seeks_missed: u64,
    pub seeks_coalesced: u64,
    pub seeks_superseded: u64,
    pub seeks_prepared: u64,
    pub decode_flushes: u64,
    pub resolve_micros: f64,
    pub present_millis: f64,
    pub decoded_frames: u64,
    pub dropped_frames: u64,
    pub decode_errors: u64,
    pub silence_skipped_seconds: f64,
    pub backend: String,
}

impl TelemetrySnapshot {
    pub fn hit_rate(&self) -> Option<f64> {
        let total = self.seeks_hit + self.seeks_missed;
        (total > 0).then(|| self.seeks_hit as f64 / total as f64)
    }
}

impl Telemetry {
    pub fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_backend(&self, backend: String) {
        *self.backend.lock() = backend;
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        TelemetrySnapshot {
            seeks_hit: load(&self.seeks_hit),
            seeks_missed: load(&self.seeks_missed),
            seeks_coalesced: load(&self.seeks_coalesced),
            seeks_superseded: load(&self.seeks_superseded),
            seeks_prepared: load(&self.seeks_prepared),
            decode_flushes: load(&self.decode_flushes),
            resolve_micros: load(&self.last_resolve_nanos) as f64 / 1_000.0,
            present_millis: load(&self.last_present_micros) as f64 / 1_000.0,
            decoded_frames: load(&self.decoded_frames),
            dropped_frames: load(&self.dropped_frames),
            decode_errors: load(&self.decode_errors),
            silence_skipped_seconds: load(&self.silence_skipped_micros) as f64 / 1e6,
            backend: self.backend.lock().clone(),
        }
    }
}
