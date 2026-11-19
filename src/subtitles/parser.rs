use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::media::Micros;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    pub start: Micros,
    pub end: Micros,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Ass,
}

impl SubtitleFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "srt" => Some(Self::Srt),
            "vtt" => Some(Self::Vtt),
            "ass" | "ssa" => Some(Self::Ass),
            _ => None,
        }
    }
}

pub fn parse_timestamp(text: &str) -> Option<Micros> {
    let mut seconds = 0f64;
    let mut parts = 0;
    for part in text.trim().split(':') {
        let value: f64 = part.trim().replace(',', ".").parse().ok()?;
        if value < 0.0 {
            return None;
        }
        seconds = seconds * 60.0 + value;
        parts += 1;
    }
    (2..=3)
        .contains(&parts)
        .then(|| (seconds * 1e6).round() as Micros)
}

pub fn strip_markup(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut closing = None;
    for character in text.chars() {
        match (closing, character) {
            (Some(end), current) if current == end => closing = None,
            (Some(_), _) => {}
            (None, '<') => closing = Some('>'),
            (None, '{') => closing = Some('}'),
            (None, current) => clean.push(current),
        }
    }
    clean
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn clean_ass_text(text: &str) -> String {
    let unescaped = text
        .replace("\\N", "\n")
        .replace("\\n", "\n")
        .replace("\\h", " ");
    strip_markup(&unescaped)
}

pub fn embedded_dialogue_text(event: &str) -> Option<String> {
    let text = event.splitn(9, ',').nth(8)?;
    Some(clean_ass_text(text)).filter(|text| !text.is_empty())
}

fn parse_blocks(content: &str) -> Vec<Cue> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let mut lines = block.lines().skip_while(|line| !line.contains("-->"));
        let Some(timing) = lines.next() else {
            continue;
        };
        let Some((start, rest)) = timing.split_once("-->") else {
            continue;
        };
        let end = rest.split_whitespace().next().unwrap_or_default();
        let (Some(start), Some(end)) = (parse_timestamp(start), parse_timestamp(end)) else {
            continue;
        };
        let text = strip_markup(&lines.collect::<Vec<_>>().join("\n"));
        if end > start && !text.is_empty() {
            cues.push(Cue { start, end, text });
        }
    }
    cues
}

fn parse_ass(content: &str) -> Vec<Cue> {
    let mut in_events = false;
    let mut fields: Vec<String> = [
        "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
    ]
    .map(str::to_owned)
    .into();
    let mut cues = Vec::new();
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(format) = line.strip_prefix("Format:") {
            fields = format
                .split(',')
                .map(|field| field.trim().to_ascii_lowercase())
                .collect();
            continue;
        }
        let Some(event) = line.strip_prefix("Dialogue:") else {
            continue;
        };
        let values: Vec<&str> = event.splitn(fields.len(), ',').collect();
        let value = |name: &str| {
            fields
                .iter()
                .position(|field| field == name)
                .and_then(|index| values.get(index))
                .copied()
        };
        let (Some(start), Some(end), Some(text)) = (
            value("start").and_then(parse_timestamp),
            value("end").and_then(parse_timestamp),
            value("text").map(clean_ass_text),
        ) else {
            continue;
        };
        if end > start && !text.is_empty() {
            cues.push(Cue { start, end, text });
        }
    }
    cues
}

pub fn parse(content: &str, format: SubtitleFormat) -> Vec<Cue> {
    let content = content.trim_start_matches('\u{feff}');
    let mut cues = match format {
        SubtitleFormat::Srt | SubtitleFormat::Vtt => parse_blocks(content),
        SubtitleFormat::Ass => parse_ass(content),
    };
    cues.sort_by_key(|cue| (cue.start, cue.end));
    cues
}

fn decode(bytes: &[u8]) -> String {
    if let Some(utf16) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = utf16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|byte| char::from(*byte)).collect(),
    }
}

pub fn parse_file(path: &Path) -> Result<Vec<Cue>> {
    let Some(format) = SubtitleFormat::from_path(path) else {
        bail!("{} is not a supported subtitle format", path.display());
    };
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let cues = parse(&decode(&bytes), format);
    if cues.is_empty() {
        bail!("{} contains no subtitle cues", path.display());
    }
    Ok(cues)
}
