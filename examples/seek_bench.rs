use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use veloce::{
    demo::SyntheticSource,
    engine::{
        CacheLimits, FfmpegSource, OpenOptions, PipelineEvent, SeekOutcome, SessionShared,
        ffmpeg_source, pipeline,
    },
    media::{MICROS_PER_SECOND, MediaInfo, MediaSource, Micros},
};

const SAMPLE_RATE: u32 = 48_000;
const IN_WINDOW_SEEKS: usize = 10_000;
const FAR_SEEKS: Micros = 12;
const BURST_SEEKS: Micros = 40;
const KEY_REPEAT: Duration = Duration::from_millis(15);

fn wait_for(what: &str, limit: Duration, condition: impl Fn() -> bool) -> Result<Duration> {
    let started = Instant::now();
    while !condition() {
        if started.elapsed() > limit {
            bail!("timed out waiting for {what}");
        }
        thread::sleep(Duration::from_micros(200));
    }
    Ok(started.elapsed())
}

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * fraction) as usize]
}

fn open(location: Option<String>, shared: &Arc<SessionShared>) -> Result<MediaInfo> {
    let (events, inbox) = crossbeam_channel::unbounded();
    let cancel = shared.clone();
    pipeline::spawn(
        shared.clone(),
        Box::new(move || match location {
            Some(location) => {
                let options = OpenOptions {
                    sample_rate: SAMPLE_RATE,
                    norm16_textures: true,
                };
                let source = FfmpegSource::open(&location, options, move || cancel.is_stopped())?;
                Ok(Box::new(source) as Box<dyn MediaSource>)
            }
            None => Ok(Box::new(SyntheticSource::new(SAMPLE_RATE)) as Box<dyn MediaSource>),
        }),
        events,
    )?;
    match inbox.recv_timeout(Duration::from_secs(30)) {
        Ok(PipelineEvent::Opened(info)) => Ok(*info),
        Ok(PipelineEvent::Failed(message)) => bail!(message),
        _ => bail!("the source did not open in time"),
    }
}

fn main() -> Result<()> {
    let mut location = None;
    let mut limits = CacheLimits::default();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--demo" => location = None,
            "--cache-mb" => {
                let megabytes: usize = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .context("--cache-mb expects a number")?;
                limits.budget_bytes = megabytes * 1024 * 1024;
            }
            _ => location = Some(argument),
        }
    }
    ffmpeg_source::initialize()?;
    let shared = Arc::new(SessionShared::new(limits, SAMPLE_RATE, 0));
    let info = open(location, &shared)?;
    let video = info
        .video
        .as_ref()
        .context("the source has no video stream")?;
    let duration = info.duration.context("the source has no known duration")?;
    println!("source        {} ({})", info.title, info.container);
    println!(
        "video         {} {}x{} @ {:.2} fps",
        video.codec, video.width, video.height, video.frame_rate
    );

    let fill = wait_for("the window to fill", Duration::from_secs(60), || {
        !shared.cache.wants_more(0) || shared.end_of_stream()
    })?;
    let span = shared.cache.span().context("no frames were decoded")?;
    let telemetry = shared.telemetry.snapshot();
    let decode_rate = telemetry.decoded_frames as f64 / fill.as_secs_f64();
    println!("decoder       {}", telemetry.backend);
    println!(
        "window        {:.1} s ahead, {} frames, {:.0} MiB of {:.0} MiB",
        (span.end - span.start) as f64 / 1e6,
        span.frames,
        span.bytes as f64 / 1_048_576.0,
        limits.budget_bytes as f64 / 1_048_576.0
    );
    println!(
        "fill rate     {decode_rate:.0} fps ({:.1}x realtime)",
        decode_rate / video.frame_rate.max(1.0)
    );

    let hop = (5 * MICROS_PER_SECOND).min((span.end - span.start) / 3);
    let base = span.start + hop / 2;
    let flushes = shared.telemetry.snapshot().decode_flushes;
    let mut latencies = Vec::with_capacity(IN_WINDOW_SEEKS);
    for step in 0..IN_WINDOW_SEEKS {
        let target = base + if step % 2 == 0 { hop } else { 0 };
        let started = Instant::now();
        let (outcome, _) = shared.seek(target);
        let frame = shared.cache.frame_at(target);
        latencies.push(started.elapsed());
        if outcome != SeekOutcome::Hit || frame.is_none() {
            bail!("in-window seek to {target} missed the ring buffer");
        }
    }
    latencies.sort_unstable();
    println!(
        "in-window     {IN_WINDOW_SEEKS} seeks of {:.1} s: median {:?}, p99 {:?}, max {:?}, {} decoder flushes",
        hop as f64 / 1e6,
        percentile(&latencies, 0.5),
        percentile(&latencies, 0.99),
        percentile(&latencies, 1.0),
        shared.telemetry.snapshot().decode_flushes - flushes
    );

    let mut misses = Vec::new();
    for step in 1..=FAR_SEEKS {
        let target = duration * ((step * 5) % (FAR_SEEKS + 1)) / (FAR_SEEKS + 1);
        let started = Instant::now();
        let (outcome, epoch) = shared.seek(target);
        if outcome == SeekOutcome::Hit {
            continue;
        }
        wait_for("an out-of-window seek", Duration::from_secs(30), || {
            shared.is_ready(epoch)
        })?;
        misses.push(started.elapsed());
    }
    misses.sort_unstable();
    if !misses.is_empty() {
        println!(
            "out-of-window {} seeks: median {:?}, slowest {:?}",
            misses.len(),
            percentile(&misses, 0.5),
            percentile(&misses, 1.0)
        );
    }

    let before = shared.telemetry.snapshot();
    let started = Instant::now();
    let mut last_epoch = 0;
    for step in 1..=BURST_SEEKS {
        let target = (step * 30 * MICROS_PER_SECOND) % (duration - MICROS_PER_SECOND).max(1);
        last_epoch = shared.seek(target).1;
        thread::sleep(KEY_REPEAT);
    }
    wait_for("the burst to settle", Duration::from_secs(30), || {
        shared.is_ready(last_epoch)
    })?;
    let after = shared.telemetry.snapshot();
    println!(
        "30 s key spam {BURST_SEEKS} seeks in {:?}: {} decoded to completion, {} preempted, {} merged",
        started.elapsed(),
        BURST_SEEKS as u64
            - (after.seeks_superseded - before.seeks_superseded)
            - (after.seeks_coalesced - before.seeks_coalesced),
        after.seeks_superseded - before.seeks_superseded,
        after.seeks_coalesced - before.seeks_coalesced
    );
    shared.stop();
    Ok(())
}
