use std::path::Path;

use anyhow::Result;

use super::command::SubtitleSelection;
use crate::{
    media::{MediaInfo, Micros, RawSubtitle, SubtitlePayload},
    subtitles::{
        Cue, SubtitleStyle, SubtitleTrack,
        parser::{self, embedded_dialogue_text, strip_markup},
    },
};

const SIDECAR_EXTENSIONS: [&str; 4] = ["srt", "ass", "ssa", "vtt"];

#[derive(Default)]
pub struct SubtitleState {
    embedded: Vec<(String, bool, SubtitleTrack)>,
    external: Vec<(String, SubtitleTrack)>,
    pub selection: Option<SubtitleSelection>,
    pub delay: Micros,
    pub style: SubtitleStyle,
}

impl SubtitleState {
    pub fn reset(&mut self) {
        self.embedded.clear();
        self.external.clear();
        self.selection = None;
    }

    pub fn adopt_embedded(&mut self, info: &MediaInfo) {
        self.embedded = info
            .subtitle_tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                (
                    track.label(index),
                    track.text_based,
                    SubtitleTrack::default(),
                )
            })
            .collect();
    }

    pub fn ingest(&mut self, subtitle: RawSubtitle) {
        let text = match &subtitle.payload {
            SubtitlePayload::Plain(text) => Some(strip_markup(text)),
            SubtitlePayload::AssDialogue(event) => embedded_dialogue_text(event),
        };
        if let (Some(text), Some((_, _, track))) = (text, self.embedded.get_mut(subtitle.track)) {
            track.insert(Cue {
                start: subtitle.start,
                end: subtitle.end,
                text,
            });
        }
    }

    pub fn load_external(&mut self, path: &Path) -> Result<String> {
        let cues = parser::parse_file(path)?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.external.retain(|(existing, _)| *existing != name);
        self.external
            .push((name.clone(), SubtitleTrack::from_cues(cues)));
        self.selection = Some(SubtitleSelection::External(self.external.len() - 1));
        Ok(name)
    }

    pub fn load_sidecars(&mut self, media_path: &Path) {
        for extension in SIDECAR_EXTENSIONS {
            let candidate = media_path.with_extension(extension);
            if candidate.is_file()
                && let Err(error) = self.load_external(&candidate)
            {
                log::warn!("ignoring sidecar subtitles: {error:#}");
            }
        }
    }

    pub fn choices(&self) -> Vec<(SubtitleSelection, String)> {
        let embedded = self
            .embedded
            .iter()
            .enumerate()
            .filter(|(_, (_, text_based, _))| *text_based)
            .map(|(index, (label, _, _))| (SubtitleSelection::Embedded(index), label.clone()));
        let external = self
            .external
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (SubtitleSelection::External(index), name.clone()));
        embedded.chain(external).collect()
    }

    pub fn label(&self) -> String {
        self.choices()
            .into_iter()
            .find(|(selection, _)| Some(*selection) == self.selection)
            .map_or_else(|| "Off".to_owned(), |(_, label)| label)
    }

    pub fn cycle(&mut self) {
        let choices = self.choices();
        let position = choices
            .iter()
            .position(|(selection, _)| Some(*selection) == self.selection);
        self.selection = match position {
            None => choices.first().map(|(selection, _)| *selection),
            Some(index) => choices.get(index + 1).map(|(selection, _)| *selection),
        };
    }

    fn track(&self) -> Option<&SubtitleTrack> {
        match self.selection? {
            SubtitleSelection::Embedded(index) => self.embedded.get(index).map(|entry| &entry.2),
            SubtitleSelection::External(index) => self.external.get(index).map(|entry| &entry.1),
        }
    }

    pub fn lines_at(&self, position: Micros) -> Vec<String> {
        self.track().map_or_else(Vec::new, |track| {
            track
                .active_at(position - self.delay)
                .map(|cue| cue.text.clone())
                .collect()
        })
    }
}
