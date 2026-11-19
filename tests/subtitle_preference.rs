use std::{
    path::{Path, PathBuf},
    process::Command as Process,
    thread,
    time::{Duration, Instant},
};

use veloce::{
    engine::CacheLimits,
    gpu::GpuCapabilities,
    player::{Command, Player},
    subtitles::{TrackSignature, preference::normalize_language},
};

fn embedded(language: Option<&str>, title: Option<&str>) -> TrackSignature {
    TrackSignature::embedded(language, title)
}

#[test]
fn language_codes_and_names_share_one_identity() {
    for code in ["fr", "fre", "FRA", "French", "français"] {
        assert_eq!(normalize_language(code), Some("fr"), "{code}");
    }
    assert_eq!(normalize_language("und"), None);
    assert_eq!(
        embedded(Some("und"), Some("English SDH")).language,
        Some("en")
    );
}

#[test]
fn external_names_are_read_after_the_media_stem() {
    let tagged = TrackSignature::external("Show.S01E02.fr.forced", Some("Show.S01E02"));
    assert_eq!(tagged.language, Some("fr"));
    assert_eq!(tagged.tags, vec!["forced"]);
    let plain = TrackSignature::external("Show.S01E02", Some("Show.S01E02"));
    assert_eq!(plain.language, None);
    let dropped = TrackSignature::external("Is.It.Real.Love", None);
    assert_eq!(dropped.language, None);
}

#[test]
fn the_closest_track_wins_and_other_languages_never_match() {
    let wanted = embedded(Some("fre"), Some("Full"));
    let mut candidates = vec![
        embedded(Some("eng"), None),
        embedded(Some("fra"), Some("Forced")),
        embedded(Some("fra"), Some("Full")),
    ];
    TrackSignature::assign_ordinals(&mut candidates);
    assert_eq!(wanted.best_match(&candidates), Some(2));
    assert_eq!(wanted.best_match(&candidates[..1]), None);

    let forced = embedded(Some("fre"), Some("Forced"));
    let full = TrackSignature::external("Movie.fr", Some("Movie"));
    let forced_file = TrackSignature::external("Movie.fr.forced", Some("Movie"));
    let mut files = vec![full, forced_file];
    TrackSignature::assign_ordinals(&mut files);
    assert_eq!(forced.best_match(&files), Some(1));
}

#[test]
fn tracks_without_language_match_by_title_or_position() {
    let mut candidates = vec![embedded(None, Some("Director")), embedded(None, None)];
    TrackSignature::assign_ordinals(&mut candidates);
    assert_eq!(
        embedded(None, Some("director")).best_match(&candidates),
        Some(0)
    );
    let mut second = embedded(None, None);
    second.ordinal = 1;
    assert_eq!(second.best_match(&candidates), Some(1));
    second.ordinal = 4;
    assert_eq!(second.best_match(&candidates), None);
}

fn write_srt(path: &Path, text: &str) -> Option<()> {
    std::fs::write(path, format!("1\n00:00:00,000 --> 00:00:02,000\n{text}\n")).ok()
}

/// A short clip whose subtitle streams carry the given languages, in order.
fn clip(directory: &Path, name: &str, languages: &[&str]) -> Option<PathBuf> {
    let path = directory.join(format!("{name}.mkv"));
    let mut process = Process::new("ffmpeg");
    process
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=160x90:rate=30:duration=2")
        .args(["-f", "lavfi", "-i", "sine=sample_rate=48000:duration=2"]);
    for language in languages {
        let srt = directory.join(format!("{name}-{language}.srt"));
        write_srt(&srt, language)?;
        process.arg("-i").arg(srt);
    }
    process.args(["-map", "0:v", "-map", "1:a"]);
    for (index, language) in languages.iter().enumerate() {
        process
            .args(["-map", &format!("{}:s", index + 2)])
            .arg(format!("-metadata:s:s:{index}"))
            .arg(format!("language={language}"));
    }
    process.args(["-c:v", "mpeg4", "-c:a", "aac", "-c:s", "srt"]);
    process.arg(&path).status().ok()?.success().then_some(path)
}

fn player() -> Player {
    let capabilities = GpuCapabilities {
        norm16_textures: true,
        max_texture_side: 8192,
    };
    let mut player = Player::new(capabilities, CacheLimits::default());
    player.execute(Command::SetVolume(0.0));
    player
}

fn open_until(player: &mut Player, path: &Path, choices: usize) -> String {
    player.execute(Command::Open(vec![path.display().to_string()]));
    let deadline = Instant::now() + Duration::from_secs(10);
    while player.subtitles.choices().len() < choices || player.is_loading() {
        player.update(Duration::from_millis(8));
        assert!(Instant::now() < deadline, "{} never opened", path.display());
        thread::sleep(Duration::from_millis(8));
    }
    player.subtitles.label()
}

#[test]
fn the_next_file_reuses_the_language_chosen_in_the_previous_one() {
    let directory = std::env::temp_dir().join(format!("veloce-subpref-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temporary directory");
    let (Some(first), Some(second), Some(bare)) = (
        clip(&directory, "first", &["eng", "fre"]),
        clip(&directory, "second", &["fre", "spa", "eng"]),
        clip(&directory, "bare", &[]),
    ) else {
        eprintln!("ffmpeg binary unavailable, skipping");
        return;
    };
    let mut player = player();
    assert_eq!(open_until(&mut player, &first, 2), "Off");
    player.execute(Command::CycleSubtitles);
    player.execute(Command::CycleSubtitles);
    assert_eq!(player.subtitles.label(), "Subtitle 2 [fre]");

    assert_eq!(open_until(&mut player, &second, 3), "Subtitle 1 [fre]");

    write_srt(&directory.join("bare.en.srt"), "hello").expect("sidecar");
    write_srt(&directory.join("bare.fr.srt"), "bonjour").expect("sidecar");
    assert_eq!(open_until(&mut player, &bare, 2), "bare.fr.srt");

    player.execute(Command::SelectSubtitles(None));
    assert_eq!(open_until(&mut player, &bare, 2), "Off");
    let _ = std::fs::remove_dir_all(directory);
}
