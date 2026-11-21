use std::path::Path;

use anyhow::Result;

use super::command::SubtitleSelection;
use crate::{
    media::{MediaInfo, Micros, RawSubtitle, SubtitlePayload},
    subtitles::{
        Cue, SubtitlePreference as Preference, SubtitleStyle, SubtitleTrack, TrackSignature,
        parser::{self, embedded_dialogue_text, strip_markup},
        sidecar::{self, stem_of},
    },
};

struct Embedded {
    label: String,
    text_based: bool,
    signature: TrackSignature,
    track: SubtitleTrack,
}

struct External {
    name: String,
    signature: TrackSignature,
    track: SubtitleTrack,
}

#[derive(Default)]
pub struct SubtitleState {
    embedded: Vec<Embedded>,
    external: Vec<External>,
    media_stem: Option<String>,
    preference: Preference,
    automatic: bool,
    pub selection: Option<SubtitleSelection>,
    pub delay: Micros,
    pub style: SubtitleStyle,
}

impl SubtitleState {
    pub fn reset(&mut self) {
        if let Some(signature) = self.selected_signature() {
            self.preference = Preference::Track(signature);
        }
        self.embedded.clear();
        self.external.clear();
        self.media_stem = None;
        self.selection = None;
        self.automatic = true;
    }

    /// The choice to carry into the next file: the track on screen, otherwise the last choice.
    pub fn preference(&self) -> Preference {
        self.selected_signature()
            .map_or_else(|| self.preference.clone(), Preference::Track)
    }

    /// Restores a choice saved by an earlier run; the next file opened honours it.
    pub fn restore(&mut self, preference: Preference) {
        self.preference = preference;
    }

    /// Returns true when a track resembling the previous file's choice was selected.
    pub fn adopt_embedded(&mut self, info: &MediaInfo) -> bool {
        self.embedded = info
            .subtitle_tracks
            .iter()
            .enumerate()
            .map(|(index, track)| Embedded {
                label: track.label(index),
                text_based: track.text_based,
                signature: TrackSignature::embedded(
                    track.language.as_deref(),
                    track.title.as_deref(),
                ),
                track: SubtitleTrack::default(),
            })
            .collect();
        self.settle()
    }

    pub fn ingest(&mut self, subtitle: RawSubtitle) {
        let text = match &subtitle.payload {
            SubtitlePayload::Plain(text) => Some(strip_markup(text)),
            SubtitlePayload::AssDialogue(event) => embedded_dialogue_text(event),
        };
        if let (Some(text), Some(entry)) = (text, self.embedded.get_mut(subtitle.track)) {
            entry.track.insert(Cue {
                start: subtitle.start,
                end: subtitle.end,
                text,
            });
        }
    }

    fn add_external(&mut self, path: &Path) -> Result<String> {
        let cues = parser::parse_file(path)?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let stem = stem_of(path).unwrap_or_else(|| name.clone());
        let signature = TrackSignature::external(&stem, self.media_stem.as_deref());
        self.external.retain(|entry| entry.name != name);
        self.external.push(External {
            name: name.clone(),
            signature,
            track: SubtitleTrack::from_cues(cues),
        });
        Ok(name)
    }

    pub fn load_external(&mut self, path: &Path) -> Result<String> {
        let name = self.add_external(path)?;
        self.select(Some(SubtitleSelection::External(self.external.len() - 1)));
        Ok(name)
    }

    /// Loads `Movie.srt` as well as tagged siblings such as `Movie.fr.forced.srt`.
    pub fn load_sidecars(&mut self, media_path: &Path) {
        self.media_stem = stem_of(media_path);
        for path in sidecar::sidecars(media_path) {
            if let Err(error) = self.add_external(&path) {
                log::warn!("ignoring sidecar subtitles: {error:#}");
            }
        }
        self.settle();
    }

    fn signatures(&self) -> Vec<(SubtitleSelection, TrackSignature)> {
        let embedded = self
            .embedded
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.text_based)
            .map(|(index, entry)| (SubtitleSelection::Embedded(index), entry.signature.clone()));
        let external =
            self.external.iter().enumerate().map(|(index, entry)| {
                (SubtitleSelection::External(index), entry.signature.clone())
            });
        let (selections, mut signatures): (Vec<_>, Vec<_>) = embedded.chain(external).unzip();
        TrackSignature::assign_ordinals(&mut signatures);
        selections.into_iter().zip(signatures).collect()
    }

    fn selected_signature(&self) -> Option<TrackSignature> {
        let selection = self.selection?;
        self.signatures()
            .into_iter()
            .find_map(|(candidate, signature)| (candidate == selection).then_some(signature))
    }

    fn settle(&mut self) -> bool {
        if !self.automatic {
            return false;
        }
        let fallback = (!self.external.is_empty()).then_some(SubtitleSelection::External(0));
        let (selection, matched) = match &self.preference {
            Preference::Off => (None, false),
            Preference::Unset => (fallback, false),
            Preference::Track(wanted) => {
                let (selections, candidates): (Vec<_>, Vec<_>) =
                    self.signatures().into_iter().unzip();
                match wanted.best_match(&candidates) {
                    Some(index) => (Some(selections[index]), true),
                    None => (fallback, false),
                }
            }
        };
        self.selection = selection;
        matched
    }

    pub fn select(&mut self, selection: Option<SubtitleSelection>) {
        self.selection = selection;
        self.automatic = false;
        self.preference = self
            .selected_signature()
            .map_or(Preference::Off, Preference::Track);
    }

    pub fn choices(&self) -> Vec<(SubtitleSelection, String)> {
        let embedded = self
            .embedded
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.text_based)
            .map(|(index, entry)| (SubtitleSelection::Embedded(index), entry.label.clone()));
        let external = self
            .external
            .iter()
            .enumerate()
            .map(|(index, entry)| (SubtitleSelection::External(index), entry.name.clone()));
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
        self.select(match position {
            None => choices.first().map(|(selection, _)| *selection),
            Some(index) => choices.get(index + 1).map(|(selection, _)| *selection),
        });
    }

    fn track(&self) -> Option<&SubtitleTrack> {
        match self.selection? {
            SubtitleSelection::Embedded(index) => self.embedded.get(index).map(|e| &e.track),
            SubtitleSelection::External(index) => self.external.get(index).map(|e| &e.track),
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
