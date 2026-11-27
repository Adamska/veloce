use std::{
    fs::File,
    path::{Path, PathBuf},
    process::Command as Process,
    thread,
    time::{Duration, Instant},
};

use veloce::{
    app::logging,
    engine::{CacheLimits, ffmpeg_source},
    gpu::{FrameSelection, GpuCapabilities},
    media::{MICROS_PER_SECOND, Micros},
    player::{
        Command, LastSession, Player, Resume,
        resume::{Bookmark, ResumeState},
    },
};

const REFRESH: Duration = Duration::from_millis(8);

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("veloce-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("temporary directory");
    directory
}

#[test]
fn positions_and_the_queue_survive_the_text_format() {
    let state = ResumeState {
        session: Some(LastSession {
            locations: vec!["/films/a b.mkv".into(), "https://host/live.m3u8".into()],
            current: 1,
            paused: true,
        }),
        bookmarks: vec![Bookmark {
            location: "/films/a b.mkv".into(),
            position: 754_321_000,
            saved: 1_790_000_000,
        }],
    };
    assert_eq!(ResumeState::parse(&state.to_text()), state);
    let damaged = "session\tx\tplaying\nqueue\t/a.mkv\nposition\tsoon\t1\t/a.mkv\nposition\t9";
    assert_eq!(ResumeState::parse(damaged), ResumeState::default());
    let out_of_range = "session\t3\tplaying\nqueue\t/a.mkv\n";
    assert_eq!(ResumeState::parse(out_of_range).session, None);
}

#[test]
fn positions_near_either_end_are_forgotten() {
    let store = scratch("resume-store").join("nested/resume");
    let mut resume = Resume::persistent(store.clone());
    let minute = 60 * MICROS_PER_SECOND;
    resume.remember("/a.mkv", 30 * MICROS_PER_SECOND, Some(minute));
    resume.remember("/b.mkv", 2 * MICROS_PER_SECOND, Some(minute));
    resume.remember("/c.mkv", minute - MICROS_PER_SECOND, Some(minute));
    resume.remember("/d.mkv", 45 * MICROS_PER_SECOND, None);
    assert_eq!(resume.position("/a.mkv"), Some(30 * MICROS_PER_SECOND));
    assert_eq!(resume.position("/b.mkv"), None);
    assert_eq!(resume.position("/c.mkv"), None);
    resume.remember("/a.mkv", 3 * MICROS_PER_SECOND, Some(minute));
    assert_eq!(resume.position("/a.mkv"), None);
    drop(resume);
    let reread = Resume::persistent(store);
    assert_eq!(reread.position("/d.mkv"), Some(45 * MICROS_PER_SECOND));
    assert_eq!(reread.position("/a.mkv"), None);
}

/// A 30 s clip made with the ffmpeg binary, or any longer video named by `VELOCE_TEST_MEDIA`.
fn sample_clip(directory: &Path) -> Option<PathBuf> {
    if let Some(media) = std::env::var_os("VELOCE_TEST_MEDIA") {
        return Some(PathBuf::from(media));
    }
    let path = directory.join("clip.mkv");
    let status = Process::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x180:rate=25:duration=30")
        .args(["-c:v", "mpeg4", "-g", "50"])
        .arg(&path)
        .status()
        .ok()?;
    status.success().then_some(path)
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

fn shows(player: &mut Player, position: Micros) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let selection: Option<FrameSelection> = player.update(REFRESH);
        if selection.is_some_and(|s| s.primary.pts <= position && position < s.primary.end()) {
            return true;
        }
        thread::sleep(REFRESH);
    }
    false
}

#[test]
fn a_relaunch_reopens_the_queue_at_the_same_frame() {
    let directory = scratch("resume-player");
    let Some(clip) = sample_clip(&directory) else {
        eprintln!("no ffmpeg binary and no VELOCE_TEST_MEDIA, skipping");
        return;
    };
    ffmpeg_source::initialize().expect("ffmpeg initializes");
    let store = directory.join("resume");
    let location = clip.to_string_lossy().into_owned();
    let stop = 12_360_000;

    let mut first = player();
    first.set_resume(Resume::persistent(store.clone()));
    assert!(!first.restore_session());
    first.execute(Command::Open(vec![location.clone()]));
    assert!(shows(&mut first, 0));
    first.execute(Command::TogglePause);
    first.execute(Command::SeekTo(stop));
    assert!(shows(&mut first, stop));
    first.save_state();
    drop(first);

    let mut second = player();
    second.set_resume(Resume::persistent(store.clone()));
    assert!(second.restore_session());
    assert!(second.is_paused());
    assert_eq!(second.position(), stop);
    assert!(shows(&mut second, stop));
    let notices = second.take_notices();
    assert!(
        notices
            .iter()
            .any(|notice| notice.text == "Resumed at 0:12")
    );
    second.execute(Command::Stop);
    second.save_state();
    drop(second);

    let mut third = player();
    third.set_resume(Resume::persistent(store));
    assert!(!third.restore_session());
    third.execute(Command::Open(vec![location]));
    assert_eq!(third.position(), stop);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn the_log_reports_runs_that_never_exited_cleanly() {
    let directory = scratch("log-markers");
    std::fs::write(directory.join("running-4000001"), "").expect("stale marker");
    let alive = File::create(directory.join("running-4000002")).expect("live marker");
    alive.lock().expect("live marker is locked");
    std::fs::write(
        directory.join(logging::FILE_NAME),
        vec![b'x'; 5 * 1024 * 1024],
    )
    .expect("oversized log");

    let (_, mut session) = logging::prepare(&directory).expect("log directory is usable");
    assert_eq!(session.unclean_exits, [4_000_001]);
    assert!(!directory.join("running-4000001").exists());
    assert!(directory.join("running-4000002").exists());
    assert!(directory.join("veloce.old.log").exists());
    let own = directory.join(format!("running-{}", std::process::id()));
    assert!(own.exists());
    session.finish();
    assert!(!own.exists());

    let (_, again) = logging::prepare(&directory).expect("log directory is usable");
    assert!(!again.crashed_before());
    drop(alive);
    let _ = std::fs::remove_dir_all(directory);
}
