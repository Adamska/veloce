use std::path::{Path, PathBuf};

use crate::{media::Micros, subtitles::SubtitleFormat};

pub const MEDIA_EXTENSIONS: [&str; 24] = [
    "mkv", "mp4", "m4v", "webm", "mov", "avi", "ts", "m2ts", "mpg", "mpeg", "flv", "wmv", "ogv",
    "3gp", "flac", "mp3", "m4a", "aac", "ogg", "opus", "wav", "wma", "m3u8", "mka",
];
const SUBTITLE_EXTENSIONS: [&str; 4] = ["srt", "vtt", "ass", "ssa"];

pub struct Dropped {
    pub media: Vec<String>,
    pub subtitles: Vec<PathBuf>,
}

pub fn classify(paths: Vec<PathBuf>) -> Dropped {
    let (subtitles, mut media): (Vec<PathBuf>, Vec<PathBuf>) = paths
        .into_iter()
        .partition(|path| SubtitleFormat::from_path(path).is_some());
    media.sort();
    Dropped {
        media: media
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        subtitles,
    }
}

pub fn pick_media() -> Option<Vec<String>> {
    let files = rfd::FileDialog::new()
        .set_title("Open media")
        .add_filter("Media", &MEDIA_EXTENSIONS)
        .add_filter("All files", &["*"])
        .pick_files()?;
    Some(classify(files).media).filter(|media| !media.is_empty())
}

pub fn pick_subtitles() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load subtitles")
        .add_filter("Subtitles", &SUBTITLE_EXTENSIONS)
        .pick_file()
}

fn sanitize(title: &str) -> String {
    let stem = Path::new(title).file_stem().map_or_else(
        || title.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "snapshot".to_owned()
    } else {
        trimmed.chars().take(80).collect()
    }
}

pub fn snapshot_path(title: &str, position: Micros) -> PathBuf {
    let millis = position.max(0) / 1_000;
    let name = format!(
        "{}-{:02}h{:02}m{:02}s{:03}.png",
        sanitize(title),
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1_000 % 60,
        millis % 1_000
    );
    std::env::home_dir()
        .map(|home| home.join("Pictures"))
        .filter(|pictures| pictures.is_dir())
        .unwrap_or_else(std::env::temp_dir)
        .join("Veloce")
        .join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_files_split_into_media_and_subtitles() {
        let dropped = classify(vec![
            PathBuf::from("/tmp/b.mkv"),
            PathBuf::from("/tmp/a.MP4"),
            PathBuf::from("/tmp/movie.en.srt"),
            PathBuf::from("/tmp/signs.ASS"),
        ]);
        assert_eq!(dropped.media, ["/tmp/a.MP4", "/tmp/b.mkv"]);
        assert_eq!(dropped.subtitles.len(), 2);
    }

    #[test]
    fn snapshot_names_are_filesystem_safe_and_timestamped() {
        let path = snapshot_path("My: Movie? (2026).mkv", 3_723_456_000);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        assert_eq!(name, "My--Movie---2026-01h02m03s456.png");
        assert!(
            path.parent()
                .is_some_and(|parent| parent.ends_with("Veloce"))
        );
        let fallback = snapshot_path("???", 0);
        assert!(fallback.ends_with("snapshot-00h00m00s000.png"));
    }
}
