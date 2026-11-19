use std::{path::PathBuf, process::Command};

use veloce::{
    engine::{FfmpegSource, OpenOptions, ffmpeg_source},
    media::{MICROS_PER_SECOND, MediaSource, Micros, SourceEvent},
};

const OPTIONS: OpenOptions = OpenOptions {
    sample_rate: 48_000,
    norm16_textures: true,
};

fn sample_clip(name: &str) -> Option<PathBuf> {
    let path = std::env::temp_dir().join(format!("veloce-test-{}-{name}.mkv", std::process::id()));
    let subtitles = path.with_extension("srt");
    std::fs::write(
        &subtitles,
        "1\n00:00:01,000 --> 00:00:02,500\nFirst <i>cue</i>\n\n2\n00:00:03,000 --> 00:00:03,800\nSecond cue\n",
    )
    .ok()?;
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x180:rate=30:duration=4")
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=4",
            "-i",
        ])
        .arg(&subtitles)
        .args(["-c:v", "mpeg4", "-g", "15", "-c:a", "aac", "-c:s", "srt"])
        .arg(&path)
        .status()
        .ok()?;
    let _ = std::fs::remove_file(subtitles);
    status.success().then_some(path)
}

struct Collected {
    video: Vec<(Micros, Micros)>,
    audio: Vec<(Micros, usize)>,
    subtitles: Vec<(Micros, Micros)>,
}

fn collect(source: &mut FfmpegSource) -> Collected {
    let mut collected = Collected {
        video: Vec::new(),
        audio: Vec::new(),
        subtitles: Vec::new(),
    };
    loop {
        match source.next_event().expect("decoding succeeds") {
            SourceEvent::Video(frame) => collected.video.push((frame.pts, frame.end())),
            SourceEvent::Audio(chunk) => collected.audio.push((chunk.pts, chunk.frames())),
            SourceEvent::Subtitle(subtitle) => {
                collected.subtitles.push((subtitle.start, subtitle.end))
            }
            SourceEvent::Idle => {}
            SourceEvent::EndOfStream => return collected,
        }
    }
}

#[test]
fn decodes_seeks_and_resamples_a_real_container() {
    let Some(path) = sample_clip("decode") else {
        eprintln!("ffmpeg binary unavailable, skipping");
        return;
    };
    ffmpeg_source::initialize().expect("ffmpeg initializes");
    let location = path.to_string_lossy().into_owned();
    let mut source = FfmpegSource::open(&location, OPTIONS, || false).expect("clip opens");

    let info = source.info().clone();
    let video = info.video.as_ref().expect("clip has video");
    assert_eq!((video.width, video.height), (320, 180));
    assert!((video.frame_rate - 30.0).abs() < 0.01);
    assert_eq!(info.audio_tracks.len(), 1);
    assert_eq!(info.subtitle_tracks.len(), 1);
    assert!(info.subtitle_tracks[0].text_based);
    let duration = info.duration.expect("clip has a duration");
    assert!((duration - 4 * MICROS_PER_SECOND).abs() < 200_000);

    let all = collect(&mut source);
    assert_eq!(all.video.len(), 120);
    assert!((0..50_000).contains(&all.video[0].0));
    assert!(all.video.windows(2).all(|pair| {
        let delta = pair[1].0 - pair[0].0;
        (33_000..=34_000).contains(&delta)
    }));
    let audio_frames: usize = all.audio.iter().map(|(_, frames)| frames).sum();
    assert!(
        (audio_frames as i64 - 4 * 48_000).abs() < 4_800,
        "{audio_frames} frames"
    );
    assert!(all.audio.windows(2).all(|pair| {
        let expected = pair[0].0 + pair[0].1 as Micros * 1_000_000 / 48_000;
        (pair[1].0 - expected).abs() < 2_000
    }));
    assert_eq!(all.subtitles.len(), 2);
    assert!((all.subtitles[0].0 - 1_000_000).abs() < 50_000);
    assert_eq!(all.subtitles[0].1 - all.subtitles[0].0, 1_500_000);

    let target = 2_700_000;
    source.seek(target).expect("seek succeeds");
    let tail = collect(&mut source);
    let first = tail.video.first().expect("frames follow the seek");
    assert!(
        first.0 <= target && first.1 > target - 300_000,
        "first frame {first:?}"
    );
    assert_eq!(
        tail.video.last().map(|frame| frame.0),
        all.video.last().map(|frame| frame.0)
    );
    assert!(
        tail.audio
            .first()
            .is_some_and(|chunk| chunk.0 <= target + 50_000)
    );

    source
        .select_audio_track(None)
        .expect("audio can be disabled");
    source.seek(0).expect("seek to start succeeds");
    let silent = collect(&mut source);
    assert_eq!(silent.video.len(), 120);
    assert!(silent.audio.is_empty());

    let _ = std::fs::remove_file(path);
}

#[test]
fn reports_a_clear_error_for_missing_files() {
    ffmpeg_source::initialize().expect("ffmpeg initializes");
    let error = FfmpegSource::open("/nonexistent/veloce.mkv", OPTIONS, || false)
        .err()
        .expect("opening must fail");
    assert!(format!("{error:#}").contains("failed to open"));
}
