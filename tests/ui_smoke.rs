mod support;

use std::{
    thread,
    time::{Duration, Instant},
};

use support::Stage;
use veloce::{
    engine::CacheLimits,
    gpu::{FrameSelection, GpuCapabilities},
    player::{Command, Player},
    ui::{Action, Interface, filters_panel::Tab},
};

fn paused_demo() -> (Player, FrameSelection) {
    let capabilities = GpuCapabilities {
        norm16_textures: false,
        max_texture_side: 8192,
    };
    let limits = CacheLimits {
        budget_bytes: 512 * 1024 * 1024,
        ..CacheLimits::default()
    };
    let mut player = Player::new(capabilities, limits);
    player.execute(Command::SetVolume(0.0));
    player.execute(Command::OpenDemo);
    let deadline = Instant::now() + Duration::from_secs(10);
    let selection = loop {
        let selection = player.update(Duration::from_millis(8));
        if player.position() > 400_000
            && let Some(selection) = selection
        {
            break selection;
        }
        assert!(Instant::now() < deadline, "the demo never produced a frame");
        thread::sleep(Duration::from_millis(8));
    };
    player.execute(Command::SetVolume(0.8));
    player.execute(Command::SeekTo(83_250_000));
    player.execute(Command::TogglePause);
    let selection = loop {
        match player.update(Duration::from_millis(8)) {
            Some(selection) if selection.primary.pts > 83_000_000 => break selection,
            _ => thread::sleep(Duration::from_millis(8)),
        }
        assert!(
            Instant::now() < deadline,
            "the demo never reached the seek target"
        );
        let _ = &selection;
    };
    player.take_notices();
    (player, selection)
}

#[test]
fn every_surface_lays_out_without_spurious_actions() {
    let Some(mut stage) = Stage::new() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let capabilities = GpuCapabilities {
        norm16_textures: false,
        max_texture_side: 8192,
    };
    let idle = Player::new(capabilities, CacheLimits::default());
    let mut interface = Interface::default();
    stage.settle(&mut interface, &idle, None);
    stage.save("01-welcome");
    interface.apply(&Action::ShowNetworkDialog);
    stage.settle(&mut interface, &idle, None);
    stage.save("02-network");
    drop(idle);

    let (mut player, selection) = paused_demo();
    let mut interface = Interface::default();
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("03-hud");

    player.execute(Command::ToggleLoopPoint);
    player.execute(Command::AdjustAudioDelay(25_000));
    interface.notify(player.take_notices(), &stage.ctx);
    interface.apply(&Action::ToggleInspector);
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("04-inspector");
    interface.apply(&Action::ToggleInspector);

    player.execute(Command::Enqueue(vec![
        "/media/Lecture 04 - Signal Processing.mkv".to_owned(),
        "https://stream.example.org/live/index.m3u8".to_owned(),
    ]));
    interface.apply(&Action::TogglePlaylist);
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("05-playlist");

    for (tab, name) in [
        (Tab::Video, "06-filters-video"),
        (Tab::Audio, "07-filters-audio"),
    ] {
        interface.apply(&Action::ShowFilters(tab));
        stage.settle(&mut interface, &player, Some(&selection));
        stage.save(name);
    }
    interface.apply(&Action::ToggleSubtitles);
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("08-subtitle-window");
    interface.apply(&Action::ToggleMoreControls);
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("10-more-controls");
    assert!(interface.has_overlay());
    assert!(interface.apply(&Action::Dismiss));
    assert!(!interface.has_overlay());

    let subtitles = std::env::temp_dir().join(format!("veloce-ui-{}.srt", std::process::id()));
    std::fs::write(
        &subtitles,
        "1\n00:01:20,000 --> 00:01:30,000\nSubtitles render above the control capsule\n<i>with a contrast backdrop</i>\n",
    )
    .expect("subtitle fixture is written");
    player.execute(Command::LoadSubtitles(subtitles.clone()));
    let _ = std::fs::remove_file(subtitles);
    assert_eq!(player.subtitles.lines_at(player.position()).len(), 1);
    player.take_notices();
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("09-subtitles");
}

#[test]
fn control_capsule_fits_the_minimum_window_width() {
    let Some(mut stage) = Stage::with_size([1088, 612]) else {
        return;
    };
    let (player, selection) = paused_demo();
    let mut interface = Interface::default();
    interface.apply(&Action::ToggleMoreControls);
    stage.settle(&mut interface, &player, Some(&selection));
    stage.save("11-narrow-expanded");
}
