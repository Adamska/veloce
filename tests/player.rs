use std::{
    thread,
    time::{Duration, Instant},
};

use veloce::{
    engine::CacheLimits,
    gpu::{FrameSelection, GpuCapabilities},
    media::{MICROS_PER_SECOND, Micros},
    player::{Command, Player},
};

const REFRESH: Duration = Duration::from_millis(8);
const FRAME: Micros = 16_666;

fn silent_player() -> Player {
    let capabilities = GpuCapabilities {
        norm16_textures: true,
        max_texture_side: 8192,
    };
    let limits = CacheLimits {
        budget_bytes: 384 * 1024 * 1024,
        ..CacheLimits::default()
    };
    let mut player = Player::new(capabilities, limits);
    player.execute(Command::SetVolume(0.0));
    player
}

#[track_caller]
fn pump(
    player: &mut Player,
    until: impl Fn(&Player, Option<&FrameSelection>) -> bool,
) -> FrameSelection {
    let caller = std::panic::Location::caller();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let selection = player.update(REFRESH);
        if until(player, selection.as_ref())
            && let Some(selection) = selection
        {
            return selection;
        }
        assert!(
            Instant::now() < deadline,
            "player stalled at {} (line {}, loading {}, span {:?}, loop {:?}, paused {})",
            player.position(),
            caller.line(),
            player.is_loading(),
            player.cache_span(),
            player.loop_points(),
            player.is_paused()
        );
        thread::sleep(REFRESH);
    }
}

fn shows(position: Micros, selection: Option<&FrameSelection>) -> bool {
    selection.is_some_and(|s| s.primary.pts <= position && position < s.primary.end())
}

#[test]
fn demo_plays_scrubs_steps_and_changes_speed() {
    let mut player = silent_player();
    assert!(!player.has_session());
    player.execute(Command::OpenDemo);

    let first = pump(&mut player, |player, frame| {
        player.position() > 200_000 && shows(player.position(), frame)
    });
    assert_eq!((first.primary.width, first.primary.height), (3840, 2160));
    let info = player.info().expect("demo metadata is published");
    assert_eq!(info.duration, Some(600 * MICROS_PER_SECOND));
    assert_eq!(info.chapters.len(), 10);

    player.execute(Command::SeekBy(1_000_000));
    let landed = player.position();
    assert!(landed > 1_150_000);
    pump(&mut player, |player, frame| shows(player.position(), frame));

    player.execute(Command::SeekTo(125 * MICROS_PER_SECOND));
    assert_eq!(player.position(), 125 * MICROS_PER_SECOND);
    let far = pump(&mut player, |player, frame| {
        !player.is_loading() && shows(player.position(), frame)
    });
    assert!(far.primary.pts >= 125 * MICROS_PER_SECOND - FRAME);

    player.execute(Command::TogglePause);
    assert!(player.is_paused());
    let held = pump(&mut player, |player, frame| shows(player.position(), frame));
    player.execute(Command::StepFrame { forward: true });
    let stepped = pump(&mut player, |player, frame| shows(player.position(), frame));
    assert_eq!(stepped.primary.pts - held.primary.pts, FRAME);
    player.execute(Command::StepFrame { forward: false });
    let back = pump(&mut player, |player, frame| shows(player.position(), frame));
    assert_eq!(back.primary.pts, held.primary.pts);

    player.execute(Command::SetSpeed(4.0));
    player.execute(Command::TogglePause);
    pump(&mut player, |player, _| {
        player.effective_speed() > 3.9 && !player.is_loading()
    });
    let settle = Instant::now();
    pump(&mut player, |_, _| {
        settle.elapsed() > Duration::from_millis(300)
    });
    let (start, started) = (player.position(), Instant::now());
    pump(&mut player, |_, _| {
        started.elapsed() > Duration::from_millis(600)
    });
    let rate = (player.position() - start) as f64 / started.elapsed().as_micros() as f64;
    assert!((rate - 4.0).abs() < 0.5, "measured {rate}x");

    let telemetry = player.telemetry().expect("session telemetry");
    assert!(telemetry.seeks_hit + telemetry.seeks_missed >= 4);
    assert!(telemetry.seeks_hit >= 2);
    assert!(telemetry.decoded_frames > 30);
    assert_eq!(telemetry.backend, "Procedural generator");
}

#[test]
fn loop_points_delays_and_tracks_respond_to_commands() {
    let mut player = silent_player();
    player.execute(Command::OpenDemo);
    pump(&mut player, |player, frame| {
        player.position() > 100_000 && shows(player.position(), frame)
    });

    player.execute(Command::AdjustAudioDelay(25_000));
    player.execute(Command::AdjustAudioDelay(25_000));
    assert_eq!(player.audio_delay(), 50_000);
    player.execute(Command::AdjustSubtitleDelay(-50_000));
    assert_eq!(player.subtitles.delay, -50_000);

    player.execute(Command::ToggleLoopPoint);
    let start = player.loop_points().0.expect("point A is set");
    pump(&mut player, |player, _| player.position() > start + 400_000);
    player.execute(Command::ToggleLoopPoint);
    let end = player.loop_points().1.expect("point B is set");
    pump(&mut player, |player, _| player.position() < end - 200_000);
    assert!(player.position() >= start);
    player.execute(Command::ToggleLoopPoint);
    assert_eq!(player.loop_points(), (None, None));

    assert_eq!(player.audio_track(), Some(0));
    player.execute(Command::CycleAudioTrack);
    assert_eq!(player.audio_track(), None);
    pump(&mut player, |player, frame| {
        !player.is_loading() && shows(player.position(), frame)
    });
    let before = player.position();
    pump(&mut player, |player, _| {
        player.position() > before + 200_000
    });

    let notices: Vec<String> = player.take_notices().into_iter().map(|n| n.text).collect();
    assert!(notices.contains(&"Audio delay +50 ms".to_owned()));
    assert!(notices.contains(&"Subtitle delay -50 ms".to_owned()));
    assert!(notices.contains(&"Loop: A-B active".to_owned()));
    assert!(notices.contains(&"Audio track: Off".to_owned()));

    player.execute(Command::Stop);
    assert!(!player.has_session());
    assert!(player.update(REFRESH).is_none());
}
