use super::frame::Micros;

#[derive(Clone, Debug, Default)]
pub struct VideoStreamInfo {
    pub codec: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub bit_rate: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct AudioTrackInfo {
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub bit_rate: Option<u64>,
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SubtitleTrackInfo {
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub text_based: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Chapter {
    pub start: Micros,
    pub end: Micros,
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub title: String,
    pub location: String,
    pub container: String,
    pub duration: Option<Micros>,
    pub seekable: bool,
    pub bit_rate: Option<u64>,
    pub video: Option<VideoStreamInfo>,
    pub audio_tracks: Vec<AudioTrackInfo>,
    pub subtitle_tracks: Vec<SubtitleTrackInfo>,
    pub chapters: Vec<Chapter>,
}

fn describe(language: Option<&str>, title: Option<&str>, fallback: String) -> String {
    match (title, language) {
        (Some(title), Some(language)) => format!("{title} [{language}]"),
        (Some(title), None) => title.to_owned(),
        (None, Some(language)) => format!("{fallback} [{language}]"),
        (None, None) => fallback,
    }
}

impl AudioTrackInfo {
    pub fn label(&self, index: usize) -> String {
        describe(
            self.language.as_deref(),
            self.title.as_deref(),
            format!("Track {}", index + 1),
        )
    }
}

impl SubtitleTrackInfo {
    pub fn label(&self, index: usize) -> String {
        describe(
            self.language.as_deref(),
            self.title.as_deref(),
            format!("Subtitle {}", index + 1),
        )
    }
}

impl MediaInfo {
    pub fn chapter_at(&self, position: Micros) -> Option<&Chapter> {
        self.chapters
            .iter()
            .rev()
            .find(|chapter| chapter.start <= position)
    }
}
