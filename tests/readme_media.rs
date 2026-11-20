//! Renders the README screenshots and the hop recording offscreen. Run through
//! `scripts/readme-media.sh`, which provides the clip and assembles the GIF.

#[allow(dead_code)]
mod support;

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};

use support::Stage;
use veloce::{
    engine::CacheLimits,
    gpu::{FrameSelection, GpuCapabilities},
    media::{MICROS_PER_SECOND, Micros},
    player::{Command, Player, RecentFiles, SubtitleSelection, recent::now_seconds},
    ui::{Action, Interface, filters_panel::Tab},
};

const REFRESH: Duration = Duration::from_millis(8);
const RECORDING_FPS: u32 = 25;

fn seconds(value: f64) -> Micros {
    (value * MICROS_PER_SECOND as f64) as Micros
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

fn wait_for(
    player: &mut Player,
    what: &str,
    done: impl Fn(&Player, &FrameSelection) -> bool,
) -> FrameSelection {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(selection) = player.update(REFRESH)
            && done(player, &selection)
        {
            return selection;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(REFRESH);
    }
}

/// Seeks while paused and returns the exact frame at `at`.
fn still(player: &mut Player, at: f64) -> FrameSelection {
    player.execute(Command::SeekTo(seconds(at)));
    let target = seconds(at);
    wait_for(player, "a still frame", |player, selection| {
        !player.is_loading() && selection.primary.pts <= target && target < selection.primary.end()
    })
}

fn shoot(
    stage: &mut Stage,
    interface: &mut Interface,
    player: &Player,
    selection: &FrameSelection,
    out: &Path,
    name: &str,
) {
    stage.settle(interface, player, Some(selection));
    stage
        .capture()
        .save_png(&out.join(format!("{name}.png")))
        .expect("screenshot is written");
}

fn welcome(stage: &mut Stage, out: &Path) {
    let root = std::env::temp_dir().join(format!("veloce-readme-{}", std::process::id()));
    let folder = root.join("Movies/Blender Open Movies");
    std::fs::create_dir_all(&folder).expect("library folder");
    let now = now_seconds();
    let mut store = String::new();
    for (name, age) in [
        ("Sintel (2010).mkv", 2 * 3_600),
        ("Tears of Steel (2012).mov", 30 * 3_600),
        ("Cosmos Laundromat (2015).mp4", 3 * 86_400),
        ("Spring (2019).mkv", 9 * 86_400),
        ("Big Buck Bunny (2008).mp4", 40 * 86_400),
    ] {
        let path = folder.join(name);
        std::fs::write(&path, b"").expect("placeholder media");
        store.push_str(&format!("{}\t{}\n", now - age, path.display()));
    }
    let store_path = root.join("recent");
    std::fs::write(&store_path, store).expect("recent store");
    // The welcome screen shortens paths under HOME to `~`.
    unsafe { std::env::set_var("HOME", &root) };
    let (checked, done) = std::sync::mpsc::channel();
    let recent = RecentFiles::persistent(store_path, move || {
        let _ = checked.send(());
    });
    done.recv_timeout(Duration::from_secs(5))
        .expect("recent files checked");
    let mut idle = player();
    idle.set_recent(recent);
    let mut interface = Interface::default();
    stage.settle(&mut interface, &idle, None);
    stage
        .capture()
        .save_png(&out.join("welcome.png"))
        .expect("screenshot is written");
    let _ = std::fs::remove_dir_all(root);
}

/// Streams rendered frames to FFmpeg at a fixed rate, repeating the latest one when rendering
/// falls behind, so the recording keeps real time.
struct Recorder {
    ffmpeg: std::process::Child,
    started: Instant,
    written: u64,
}

impl Recorder {
    fn start(size: [u32; 2], path: &Path) -> Self {
        let ffmpeg = Process::new("ffmpeg")
            .args([
                "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s",
            ])
            .arg(format!("{}x{}", size[0], size[1]))
            .args(["-r", &RECORDING_FPS.to_string(), "-i", "-"])
            .args([
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "12", "-pix_fmt", "yuv420p",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .expect("ffmpeg starts");
        Self {
            ffmpeg,
            started: Instant::now(),
            written: 0,
        }
    }

    fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn push(&mut self, rgba: &[u8]) {
        let due = (self.elapsed() * f64::from(RECORDING_FPS)) as u64 + 1;
        let stdin = self.ffmpeg.stdin.as_mut().expect("ffmpeg input");
        while self.written < due {
            stdin.write_all(rgba).expect("frame is piped");
            self.written += 1;
        }
    }

    fn finish(mut self) {
        drop(self.ffmpeg.stdin.take());
        assert!(self.ffmpeg.wait().expect("ffmpeg exits").success());
    }
}

/// Plays from `start`, presses the scheduled hops and records what the window shows.
fn record_hops(stage: &mut Stage, player: &mut Player, start: f64, out: &Path) {
    let schedule: Vec<(f64, Micros, &str)> = [1.2, 1.6, 2.0, 2.4, 2.8]
        .iter()
        .map(|at| (*at, 5 * MICROS_PER_SECOND, "→  +5 s"))
        .chain(
            [4.0, 4.4, 4.8]
                .iter()
                .map(|at| (*at, -5 * MICROS_PER_SECOND, "←  −5 s")),
        )
        .collect();
    let length = 6.2;
    still(player, start);
    player.execute(Command::TogglePause);
    wait_for(player, "playback", |player, _| {
        player.position() > seconds(start + 0.3)
    });

    let mut interface = Interface::default();
    stage.pointer_moving = true;
    let mut captions = String::new();
    let mut next = 0;
    let mut selection = None;
    let mut recorder = Recorder::start(stage.size(), &out.join("hops.mp4"));
    while recorder.elapsed() < length {
        if let Some((at, delta, label)) = schedule.get(next)
            && recorder.elapsed() >= *at
        {
            player.execute(Command::SeekBy(*delta));
            captions.push_str(&format!(
                "{:.2} {:.2} {label}\n",
                recorder.elapsed(),
                recorder.elapsed() + 0.38
            ));
            next += 1;
        }
        if let Some(latest) = player.update(REFRESH) {
            selection = Some(latest);
        }
        stage.frame(&mut interface, player, selection.as_ref());
        recorder.push(&stage.capture().rgba);
    }
    recorder.finish();
    stage.pointer_moving = false;
    std::fs::write(out.join("hops-captions.txt"), captions).expect("captions are written");
}

#[test]
#[ignore = "renders README media; run scripts/readme-media.sh"]
fn render_readme_media() {
    let clip = std::env::var("VELOCE_README_CLIP").expect("VELOCE_README_CLIP names the clip");
    let out = PathBuf::from(
        std::env::var("VELOCE_README_OUT").expect("VELOCE_README_OUT names the output"),
    );
    std::fs::create_dir_all(&out).expect("output directory");
    let mut stage = Stage::new().expect("a GPU adapter");

    welcome(&mut stage, &out);

    let mut player = player();
    player.execute(Command::Open(vec![clip.clone()]));
    player.execute(Command::TogglePause);
    wait_for(&mut player, "the clip to open", |player, _| {
        !player.is_loading()
    });

    let mut interface = Interface::default();
    let frame = still(&mut player, 95.0);
    shoot(&mut stage, &mut interface, &player, &frame, &out, "player");

    interface.apply(&Action::ShowFilters(Tab::Video));
    let frame = still(&mut player, 120.3);
    shoot(&mut stage, &mut interface, &player, &frame, &out, "filters");
    interface.apply(&Action::ShowFilters(Tab::Video));

    let english = SubtitleSelection::Embedded(1);
    player.execute(Command::SelectSubtitles(Some(english)));
    still(&mut player, 58.6);
    player.execute(Command::TogglePause);
    wait_for(&mut player, "the subtitle cue", |player, _| {
        player.position() > seconds(60.2)
    });
    player.execute(Command::TogglePause);
    let frame = still(&mut player, 60.4);
    shoot(
        &mut stage,
        &mut interface,
        &player,
        &frame,
        &out,
        "subtitles",
    );
    player.execute(Command::SelectSubtitles(None));

    record_hops(&mut stage, &mut player, 92.0, &out);

    // A fresh session, so the counters describe paused hops rather than the recording loop.
    drop(player);
    let mut player = self::player();
    player.execute(Command::Open(vec![clip]));
    player.execute(Command::TogglePause);
    wait_for(&mut player, "the clip to reopen", |player, _| {
        !player.is_loading()
    });
    still(&mut player, 146.4);
    for delta in [5, 5, -5, 5] {
        let target = player.position() + delta * MICROS_PER_SECOND;
        player.execute(Command::SeekBy(delta * MICROS_PER_SECOND));
        wait_for(&mut player, "a paused hop", |player, selection| {
            !player.is_loading()
                && selection.primary.pts <= target
                && target < selection.primary.end()
        });
        thread::sleep(Duration::from_millis(400));
    }
    let frame = still(&mut player, 156.4);
    let mut interface = Interface::default();
    interface.apply(&Action::ToggleInspector);
    shoot(
        &mut stage,
        &mut interface,
        &player,
        &frame,
        &out,
        "inspector",
    );
}
