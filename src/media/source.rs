use anyhow::Result;

use super::{
    frame::{AudioChunk, Micros, VideoFrame},
    info::MediaInfo,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubtitlePayload {
    Plain(String),
    AssDialogue(String),
}

#[derive(Clone, Debug)]
pub struct RawSubtitle {
    pub track: usize,
    pub start: Micros,
    pub end: Micros,
    pub payload: SubtitlePayload,
}

pub enum SourceEvent {
    Video(VideoFrame),
    Audio(AudioChunk),
    Subtitle(RawSubtitle),
    Idle,
    EndOfStream,
}

pub trait MediaSource {
    fn info(&self) -> &MediaInfo;

    fn decoder_backend(&self) -> String;

    fn seek(&mut self, target: Micros) -> Result<()>;

    fn selected_audio_track(&self) -> Option<usize>;

    fn select_audio_track(&mut self, track: Option<usize>) -> Result<()>;

    fn next_event(&mut self) -> Result<SourceEvent>;
}
