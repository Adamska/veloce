use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use veloce::{
    engine::{CacheLimits, FrameCache, SeekOutcome, SessionShared, pipeline},
    media::{
        BufferPool, ColorInfo, MICROS_PER_SECOND, MediaInfo, MediaSource, Micros, PixelLayout,
        PooledPlanes, SourceEvent, VideoFrame, VideoStreamInfo,
    },
};

const FRAME: Micros = 40_000;
const GOP: Micros = 2 * MICROS_PER_SECOND;

fn frame(pool: &Arc<BufferPool>, pts: Micros) -> VideoFrame {
    VideoFrame {
        pts,
        duration: FRAME,
        width: 64,
        height: 32,
        pixel_aspect: 1.0,
        layout: PixelLayout::Nv12,
        color: ColorInfo::default(),
        interlaced: false,
        storage: Box::new(PooledPlanes::new(pool, PixelLayout::Nv12, 64, 32)),
    }
}

fn limits(budget_frames: usize) -> CacheLimits {
    CacheLimits {
        behind: 10 * MICROS_PER_SECOND,
        ahead: 25 * MICROS_PER_SECOND,
        budget_bytes: budget_frames * 64 * 32 * 3 / 2,
    }
}

fn filled(budget_frames: usize, count: i64) -> FrameCache {
    let pool = Arc::new(BufferPool::default());
    let cache = FrameCache::new(limits(budget_frames));
    for index in 0..count {
        assert!(cache.push(frame(&pool, index * FRAME)));
    }
    cache
}

#[test]
fn lookup_returns_the_frame_displayed_at_a_timestamp() {
    let cache = filled(10_000, 250);
    assert_eq!(cache.frame_at(0).map(|f| f.pts), Some(0));
    assert_eq!(
        cache
            .frame_at(5 * MICROS_PER_SECOND + 39_999)
            .map(|f| f.pts),
        Some(5_000_000)
    );
    assert!(cache.frame_at(-1).is_none());
    assert!(cache.covers(249 * FRAME + 1));
    assert!(!cache.covers(250 * FRAME));
}

#[test]
fn rejects_out_of_order_frames() {
    let pool = Arc::new(BufferPool::default());
    let cache = filled(10_000, 10);
    assert!(!cache.push(frame(&pool, 3 * FRAME)));
    assert_eq!(cache.span().map(|span| span.frames), Some(10));
}

#[test]
fn window_slides_with_the_playhead() {
    let cache = filled(100_000, 1_000);
    let playhead = 30 * MICROS_PER_SECOND;
    cache.trim(playhead);
    let span = cache.span().expect("cache is populated");
    assert!(span.start >= playhead - 10 * MICROS_PER_SECOND - FRAME);
    assert!(span.start <= playhead - 10 * MICROS_PER_SECOND + FRAME);
    assert!(!cache.wants_more(playhead - 20 * MICROS_PER_SECOND));
    assert!(cache.wants_more(playhead));
}

#[test]
fn memory_budget_bounds_both_sides_of_the_window() {
    let cache = filled(70, 51);
    assert!(!cache.wants_more(0));
    cache.trim(40 * FRAME);
    let span = cache.span().expect("cache is populated");
    assert_eq!(span.start, 20 * FRAME);
    assert!(cache.wants_more(40 * FRAME));
}

#[test]
fn stepping_finds_adjacent_frames() {
    let cache = filled(10_000, 5);
    assert_eq!(
        cache.neighbor(2 * FRAME, true).map(|f| f.pts),
        Some(3 * FRAME)
    );
    assert_eq!(cache.neighbor(2 * FRAME, false).map(|f| f.pts), Some(FRAME));
    assert!(cache.neighbor(0, false).is_none());
    assert!(cache.neighbor(4 * FRAME, true).is_none());
}

#[test]
fn lookups_resolve_in_well_under_a_millisecond() {
    let cache = filled(100_000, 2_100);
    let started = Instant::now();
    let mut found = 0;
    for step in 0..10_000i64 {
        found += usize::from(cache.frame_at((step * 7_919) % (2_100 * FRAME)).is_some());
    }
    let per_lookup = started.elapsed() / 10_000;
    assert_eq!(found, 10_000);
    assert!(
        per_lookup < Duration::from_micros(50),
        "{per_lookup:?} per lookup"
    );
}

struct ScriptedSource {
    info: MediaInfo,
    pool: Arc<BufferPool>,
    next: Micros,
    seeks: Arc<std::sync::Mutex<Vec<Micros>>>,
}

impl MediaSource for ScriptedSource {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn decoder_backend(&self) -> String {
        "scripted".to_owned()
    }

    fn seek(&mut self, target: Micros) -> anyhow::Result<()> {
        self.seeks.lock().unwrap().push(target);
        std::thread::sleep(Duration::from_millis(30));
        self.next = target.div_euclid(GOP) * GOP;
        Ok(())
    }

    fn selected_audio_track(&self) -> Option<usize> {
        None
    }

    fn select_audio_track(&mut self, _track: Option<usize>) -> anyhow::Result<()> {
        Ok(())
    }

    fn next_event(&mut self) -> anyhow::Result<SourceEvent> {
        let pts = self.next;
        self.next += FRAME;
        Ok(SourceEvent::Video(frame(&self.pool, pts)))
    }
}

fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the pipeline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn seeks_inside_the_window_never_flush_and_bursts_outside_coalesce() {
    let shared = Arc::new(SessionShared::new(limits(100_000), 48_000, 0));
    let seeks = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (events, _inbox) = crossbeam_channel::unbounded();
    let source = ScriptedSource {
        info: MediaInfo {
            video: Some(VideoStreamInfo::default()),
            ..MediaInfo::default()
        },
        pool: Arc::default(),
        next: 0,
        seeks: seeks.clone(),
    };
    let worker = pipeline::spawn(
        shared.clone(),
        Box::new(move || Ok(Box::new(source))),
        events,
    )
    .expect("decode thread starts");
    wait_until(|| shared.cache.covers(20 * MICROS_PER_SECOND));
    let flushes_before = shared.telemetry.snapshot().decode_flushes;

    for step in 1..=4 {
        let (outcome, epoch) = shared.seek(step * 5 * MICROS_PER_SECOND);
        assert_eq!(outcome, SeekOutcome::Hit);
        assert!(shared.is_ready(epoch));
    }
    assert_eq!(shared.telemetry.snapshot().decode_flushes, flushes_before);
    assert!(shared.telemetry.snapshot().resolve_micros < 1_000.0);

    let mut last_epoch = 0;
    for step in 0..12 {
        let (outcome, epoch) = shared.seek((300 + step * 5) * MICROS_PER_SECOND);
        assert_eq!(outcome, SeekOutcome::Miss);
        last_epoch = epoch;
    }
    wait_until(|| shared.is_ready(last_epoch));
    let recorded = seeks.lock().unwrap().clone();
    assert!(recorded.len() <= 4, "burst was not coalesced: {recorded:?}");
    assert_eq!(recorded.last().copied(), Some(355 * MICROS_PER_SECOND));
    assert!(shared.cache.covers(355 * MICROS_PER_SECOND));
    assert!(shared.telemetry.snapshot().seeks_coalesced >= 8);

    shared.stop();
    worker.join().expect("decode thread exits cleanly");
}
