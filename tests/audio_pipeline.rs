use std::{
    f32::consts::TAU,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use veloce::{
    audio::{
        AudioControls,
        processor::{Processor, SessionSlot},
        renderer::{self, Renderer},
    },
    engine::{CacheLimits, ClockAnchor, SessionShared},
    media::{AudioChunk, MICROS_PER_SECOND, Micros},
};

const RATE: u32 = 48_000;
const BLOCK: usize = 480;
const CLICK_AT: Micros = MICROS_PER_SECOND;

struct Rig {
    session: Arc<SessionShared>,
    controls: Arc<AudioControls>,
    anchor: Arc<ClockAnchor>,
    renderer: Renderer,
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn rig(speed: f32) -> Rig {
    let session = Arc::new(SessionShared::new(CacheLimits::default(), RATE, 0));
    session.set_has_video(false);
    let mut samples = vec![0.0f32; RATE as usize * 4 * 2];
    let click_start = RATE as usize;
    for n in 0..RATE as usize * 3 {
        let value = (TAU * 1_000.0 * n as f32 / RATE as f32).sin() * 0.5;
        samples[(click_start + n) * 2] = value;
        samples[(click_start + n) * 2 + 1] = value;
    }
    session.audio.push(AudioChunk { pts: 0, samples });
    session.mark_ready(session.epoch());

    let controls = Arc::new(AudioControls::default());
    controls.paused.store(false, Ordering::Release);
    controls.set_speed(speed);
    let anchor = Arc::new(ClockAnchor::default());
    let (ring, renderer) = renderer::ring(RATE, controls.clone(), anchor.clone());
    let slot = SessionSlot::default();
    *slot.lock() = Some(session.clone());
    let shutdown = Arc::new(AtomicBool::new(false));
    let processor = Processor::new(controls.clone(), slot, ring, RATE);
    let stop = shutdown.clone();
    let worker = thread::spawn(move || processor.run(stop));
    Rig {
        session,
        controls,
        anchor,
        renderer,
        shutdown,
        worker: Some(worker),
    }
}

impl Rig {
    fn render(&mut self) -> (Vec<f32>, veloce::engine::Anchor) {
        thread::sleep(Duration::from_millis(4));
        let mut block = vec![0.0f32; BLOCK * 2];
        self.renderer.render(&mut block, 2, 0);
        (block, self.anchor.read())
    }

    fn onset_media_time(&mut self, epoch: u64) -> Micros {
        for _ in 0..2_000 {
            let (block, anchor) = self.render();
            if anchor.epoch != epoch || anchor.rate == 0.0 {
                continue;
            }
            if let Some(index) = block
                .as_chunks::<2>()
                .0
                .iter()
                .position(|frame| frame[0].abs() > 0.2)
            {
                let offset = index as f64 / f64::from(RATE) * 1e6 * f64::from(anchor.rate);
                return anchor.media + offset as Micros;
            }
        }
        panic!("the tone never reached the output");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn clock_markers_stay_aligned_with_the_audio_they_describe() {
    for speed in [1.0f32, 2.0, 0.5] {
        let mut rig = rig(speed);
        let epoch = rig.session.epoch();
        let heard_at = rig.onset_media_time(epoch);
        assert!(
            (heard_at - CLICK_AT).abs() < 12_000,
            "onset reported at {heard_at} for {speed}x"
        );
    }
}

#[test]
fn reported_rate_follows_the_playback_speed() {
    let mut rig = rig(2.0);
    let epoch = rig.session.epoch();
    let mut rates = Vec::new();
    for _ in 0..200 {
        let (_, anchor) = rig.render();
        if anchor.epoch == epoch && anchor.rate > 0.0 {
            rates.push(anchor.rate);
        }
    }
    assert!(rates.len() > 20);
    let mean = rates.iter().sum::<f32>() / rates.len() as f32;
    assert!((mean - 2.0).abs() < 0.02, "mean rate {mean}");
}

#[test]
fn seeking_discards_stale_audio_and_restarts_at_the_target() {
    let mut rig = rig(1.0);
    for _ in 0..40 {
        rig.render();
    }
    let (outcome, epoch) = rig.session.seek(2_500_000);
    assert_eq!(outcome, veloce::engine::SeekOutcome::Hit);
    let mut first = None;
    for _ in 0..400 {
        let (_, anchor) = rig.render();
        if anchor.epoch == epoch && anchor.rate > 0.0 {
            first = Some(anchor.media);
            break;
        }
    }
    let first = first.expect("audio resumes after the seek");
    assert!((first - 2_500_000).abs() < 6_000, "restarted at {first}");
}

#[test]
fn pausing_stops_consumption_and_volume_is_applied_in_the_callback() {
    let mut rig = rig(1.0);
    let epoch = rig.session.epoch();
    rig.onset_media_time(epoch);
    rig.controls.set_volume(0.5);
    let (_, _) = rig.render();
    let (block, _) = rig.render();
    let peak = block
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    assert!((peak - 0.25).abs() < 0.05, "peak {peak}");

    rig.controls.paused.store(true, Ordering::Release);
    let (block, anchor) = rig.render();
    assert!(block.iter().all(|sample| *sample == 0.0));
    assert_eq!(anchor.rate, 0.0);
}
