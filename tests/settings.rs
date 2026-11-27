use std::path::{Path, PathBuf};

use veloce::{
    engine::CacheLimits,
    gpu::GpuCapabilities,
    player::{Command, Player, Preferences, Settings, SubtitleSelection, SubtitleState},
    subtitles::{SubtitlePreference, TrackSignature},
};

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("veloce-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("temporary directory");
    directory
}

fn player() -> Player {
    let capabilities = GpuCapabilities {
        norm16_textures: true,
        max_texture_side: 8192,
    };
    Player::new(capabilities, CacheLimits::default())
}

#[test]
fn preferences_survive_the_text_format() {
    let mut track = TrackSignature::embedded(Some("fre"), Some("Forced Signs"));
    track.ordinal = 2;
    for subtitles in [
        SubtitlePreference::Unset,
        SubtitlePreference::Off,
        SubtitlePreference::Track(track.clone()),
        SubtitlePreference::Track(TrackSignature::external("Movie.en.sdh", Some("Movie"))),
    ] {
        let preferences = Preferences {
            speed: 1.3,
            volume: 0.65,
            subtitles,
        };
        assert_eq!(Preferences::parse(&preferences.to_text()), preferences);
    }
    let SubtitlePreference::Track(read) = Preferences::parse(
        &Preferences {
            subtitles: SubtitlePreference::Track(TrackSignature::embedded(
                Some("fre"),
                Some("Forced\nSigns"),
            )),
            ..Preferences::default()
        }
        .to_text(),
    )
    .subtitles
    else {
        panic!("the track preference is read back");
    };
    assert_eq!(read.language, Some("fr"));
    assert_eq!(read.tags, vec!["forced", "signs"]);
    assert_eq!(read.title.as_deref(), Some("forced signs"));
}

#[test]
fn damaged_settings_fall_back_to_defaults() {
    let preferences = Preferences::parse("speed fast\nvolume NaN\nunknown 4\n\nsubtitle-ordinal x");
    assert_eq!(preferences, Preferences::default());
}

#[test]
fn changes_are_written_at_once_then_throttled_and_flushed_on_drop() {
    let store = scratch("settings-store").join("nested/settings");
    let (mut settings, saved) = Settings::persistent(store.clone());
    assert_eq!(saved, Preferences::default());
    let faster = Preferences {
        speed: 1.25,
        ..Preferences::default()
    };
    settings.update(&faster);
    assert!(std::fs::read_to_string(&store).is_ok_and(|text| text.contains("speed 1.25")));
    let quieter = Preferences {
        volume: 0.4,
        ..faster.clone()
    };
    settings.update(&quieter);
    assert_eq!(Settings::persistent(store.clone()).1, faster);
    drop(settings);
    assert_eq!(Settings::persistent(store).1, quieter);
}

#[test]
fn speed_and_volume_are_restored_after_a_restart() {
    let store = scratch("settings-player").join("settings");
    let mut first = player();
    let (settings, saved) = Settings::persistent(store.clone());
    first.set_settings(settings, saved);
    first.execute(Command::SetVolume(0.0));
    first.execute(Command::SetSpeed(1.2));
    first.execute(Command::AdjustSpeed(0.05));
    first.save_state();
    drop(first);

    let mut second = player();
    let (settings, saved) = Settings::persistent(store);
    second.set_settings(settings, saved);
    assert_eq!(second.speed(), 1.25);
    assert_eq!(second.volume(), 0.0);
}

fn sidecar(directory: &Path, name: &str) {
    std::fs::write(
        directory.join(name),
        "1\n00:00:00,000 --> 00:00:02,000\ntext\n",
    )
    .expect("sidecar is written");
}

#[test]
fn the_subtitle_language_is_restored_after_a_restart() {
    let directory = scratch("settings-subtitles");
    for name in ["One.en.srt", "One.fr.srt", "Two.en.srt", "Two.fr.srt"] {
        sidecar(&directory, name);
    }
    let mut before = SubtitleState::default();
    before.reset();
    before.load_sidecars(&directory.join("One.mkv"));
    let french = before
        .choices()
        .into_iter()
        .find(|(_, name)| name == "One.fr.srt")
        .map(|(selection, _)| selection);
    before.select(french);
    let text = Preferences {
        subtitles: before.preference(),
        ..Preferences::default()
    }
    .to_text();

    let mut after = SubtitleState::default();
    after.restore(Preferences::parse(&text).subtitles);
    after.reset();
    after.load_sidecars(&directory.join("Two.mkv"));
    assert_eq!(after.label(), "Two.fr.srt");

    before.select(None::<SubtitleSelection>);
    assert_eq!(before.preference(), SubtitlePreference::Off);
    let _ = std::fs::remove_dir_all(directory);
}
