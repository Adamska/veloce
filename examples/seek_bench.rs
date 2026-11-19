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
        SourceFactory, ffmpeg_source,
        lanes::{self, SPARE_LANES},
        pipeline, speculation,
    },
    media::{MICROS_PER_SECOND, MediaInfo, MediaSource, Micros},
};

const SAMPLE_RATE: u32 = 48_000;
const IN_WINDOW_SEEKS: usize = 10_000;
const FAR_SEEKS: Micros = 12;
const HOPS: usize = 40;

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

fn factory(location: &Option<String>, shared: &Arc<SessionShared>, lane: usize) -> SourceFactory {
    let (location, cancel) = (location.clone(), shared.clone());
    Box::new(move || match location {
        Some(location) => {
            let options = OpenOptions {
                sample_rate: SAMPLE_RATE,
                norm16_textures: true,
                decode_threads: lanes::decode_threads(lane),
            };
            let source = FfmpegSource::open(&location, options, move || cancel.is_stopped())?;
            Ok(Box::new(source) as Box<dyn MediaSource>)
        }
        None => Ok(Box::new(SyntheticSource::new(SAMPLE_RATE)) as Box<dyn MediaSource>),
    })
}

fn open(location: &Option<String>, shared: &Arc<SessionShared>, spare: usize) -> Result<MediaInfo> {
    let (events, inbox) = crossbeam_channel::unbounded();
    pipeline::spawn(shared.clone(), factory(location, shared, 0), events.clone())?;
    for lane in 1..=spare {
        speculation::spawn_spare(
            shared.clone(),
            lane,
            factory(location, shared, lane),
            events.clone(),
        )?;
    }
    match inbox.recv_timeout(Duration::from_secs(30)) {
        Ok(PipelineEvent::Opened(info)) => Ok(*info),
        Ok(PipelineEvent::Failed(message)) => bail!(message),
        _ => bail!("the source did not open in time"),
    }
}

fn hop_run(
    shared: &SessionShared,
    start: Micros,
    stride: Micros,
    hops: usize,
    interval: Duration,
) -> Result<()> {
    let (_, epoch) = shared.seek(start);
    wait_for("the starting position", Duration::from_secs(30), || {
        shared.is_ready(epoch)
    })?;
    thread::sleep(Duration::from_millis(600));
    let (mut position, mut instant, mut late) = (start, 0, 0);
    let mut waits = Vec::new();
    let mut last_epoch = epoch;
    for _ in 0..hops {
        position += stride;
        let pressed = Instant::now();
        let (outcome, epoch) = shared.hop(position, stride);
        last_epoch = epoch;
        if outcome == SeekOutcome::Hit {
            instant += 1;
        }
        while pressed.elapsed() < interval {
            if shared.is_ready(epoch)
                && let Some(frame) = shared.cache.frame_at(position)
            {
                if frame.pts > position || position >= frame.end() + 2_000 {
                    bail!("hop to {position} showed the frame at {}", frame.pts);
                }
                waits.push(pressed.elapsed());
                break;
            }
            thread::sleep(Duration::from_micros(200));
        }
        late += usize::from(!shared.is_ready(epoch));
        thread::sleep(interval.saturating_sub(pressed.elapsed()));
    }
    wait_for("the last hop", Duration::from_secs(30), || {
        shared.is_ready(last_epoch)
    })?;
    waits.sort_unstable();
    println!(
        "{:+4} s x{hops} every {:>3} ms: {instant:>2} instant, {late:>2} not shown before the next press, median {:?}, slowest {:?}",
        stride / MICROS_PER_SECOND,
        interval.as_millis(),
        waits.get(waits.len() / 2).copied().unwrap_or_default(),
        waits.last().copied().unwrap_or_default(),
    );
    Ok(())
}

fn main() -> Result<()> {
    let mut location = None;
    let mut limits = CacheLimits::default();
    let mut spare = SPARE_LANES;
    let mut arguments = std::env::args().skip(1);
    let number = |value: Option<String>| value.and_then(|value| value.parse::<usize>().ok());
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--demo" => location = None,
            "--cache-mb" => {
                let megabytes = number(arguments.next()).context("--cache-mb expects a number")?;
                limits.budget_bytes = megabytes * 1024 * 1024;
            }
            "--lanes" => spare = number(arguments.next()).context("--lanes expects a number")?,
            _ => location = Some(argument),
        }
    }
    ffmpeg_source::initialize()?;
    let shared = Arc::new(SessionShared::with_spare_lanes(
        limits,
        SAMPLE_RATE,
        0,
        spare,
    ));
    let info = open(&location, &shared, spare)?;
    let video = info
        .video
        .as_ref()
        .context("the source has no video stream")?;
    let duration = info.duration.context("the source has no known duration")?;
    println!("source        {} ({})", info.title, info.container);
    println!(
        "video         {} {}x{} @ {:.2} fps, {spare} speculative decoders",
        video.codec, video.width, video.height, video.frame_rate
    );

    let fill = wait_for("the window to fill", Duration::from_secs(60), || {
        !shared.cache.wants_more(0) || shared.end_of_stream()
    })?;
    let span = shared.cache.span().context("no frames were decoded")?;
    let telemetry = shared.telemetry.snapshot();
    let rate = telemetry.decoded_frames as f64 / fill.as_secs_f64();
    println!("decoder       {}", telemetry.backend);
    println!(
        "window        {:.1} s ahead, {} frames, {:.0} MiB",
        (span.end - span.start) as f64 / 1e6,
        span.frames,
        span.bytes as f64 / 1_048_576.0
    );
    println!(
        "fill rate     {rate:.0} fps ({:.1}x realtime)",
        rate / video.frame_rate.max(1.0)
    );

    let hop = (5 * MICROS_PER_SECOND).min((span.end - span.start) / 3);
    let base = span.start + hop / 2;
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
        "in-window     {IN_WINDOW_SEEKS} seeks: median {:?}, p99 {:?}",
        percentile(&latencies, 0.5),
        percentile(&latencies, 0.99)
    );

    let mut misses = Vec::new();
    for step in 1..=FAR_SEEKS {
        let target = duration * ((step * 5) % (FAR_SEEKS + 1)) / (FAR_SEEKS + 1);
        let started = Instant::now();
        let (outcome, epoch) = shared.seek(target);
        if outcome == SeekOutcome::Miss {
            wait_for("a far seek", Duration::from_secs(30), || {
                shared.is_ready(epoch)
            })?;
            misses.push(started.elapsed());
        }
    }
    misses.sort_unstable();
    if !misses.is_empty() {
        println!(
            "random seeks  {} cold: median {:?}, slowest {:?}",
            misses.len(),
            percentile(&misses, 0.5),
            percentile(&misses, 1.0)
        );
    }

    let second = MICROS_PER_SECOND;
    for (stride, millis) in [(5, 90), (5, 33), (-5, 90), (30, 90), (30, 33)] {
        let stride = stride * second;
        let hops = HOPS.min(((duration - 4 * second) / stride.abs()) as usize);
        let start = if stride > 0 {
            second
        } else {
            duration - 2 * second
        };
        hop_run(&shared, start, stride, hops, Duration::from_millis(millis))?;
    }
    shared.stop();
    Ok(())
}
