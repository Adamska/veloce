//! Describes a subtitle track well enough to find its counterpart in the next file.

const LANGUAGES: &[&[&str]] = &[
    &["en", "eng", "english"],
    &["fr", "fre", "fra", "french", "francais", "français"],
    &["es", "spa", "spanish", "espanol", "español", "castellano"],
    &["de", "ger", "deu", "german", "deutsch"],
    &["it", "ita", "italian", "italiano"],
    &["pt", "por", "portuguese", "portugues", "português"],
    &["nl", "dut", "nld", "dutch", "nederlands"],
    &["ja", "jpn", "japanese"],
    &["zh", "chi", "zho", "chinese"],
    &["ko", "kor", "korean"],
    &["ru", "rus", "russian"],
    &["ar", "ara", "arabic"],
    &["sv", "swe", "swedish"],
    &["no", "nor", "nob", "norwegian"],
    &["da", "dan", "danish"],
    &["fi", "fin", "finnish"],
    &["pl", "pol", "polish"],
    &["tr", "tur", "turkish"],
    &["he", "heb", "hebrew"],
    &["hin", "hindi"],
    &["el", "gre", "ell", "greek"],
    &["cs", "cze", "ces", "czech"],
    &["hu", "hun", "hungarian"],
    &["ro", "rum", "ron", "romanian"],
    &["uk", "ukr", "ukrainian"],
    &["vi", "vie", "vietnamese"],
    &["th", "tha", "thai"],
    &["id", "ind", "indonesian"],
];

const TAGS: &[(&str, &[&str])] = &[
    ("forced", &["forced", "foreign"]),
    ("sdh", &["sdh", "cc", "hi", "hoh", "deaf"]),
    ("signs", &["signs", "songs"]),
    ("commentary", &["commentary"]),
];

/// Tokens of an external file name trusted to carry a language when the name does not start
/// with the media stem (a dropped file such as `Is.It.Real.srt` must not read as Italian).
const TRAILING_TOKENS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackSignature {
    pub language: Option<&'static str>,
    pub tags: Vec<&'static str>,
    pub title: Option<String>,
    pub external: bool,
    /// Position among the candidates of the same kind and language.
    pub ordinal: usize,
}

pub fn normalize_language(code: &str) -> Option<&'static str> {
    let code = code.trim().to_lowercase();
    LANGUAGES
        .iter()
        .find(|aliases| aliases.contains(&code.as_str()))
        .map(|aliases| aliases[0])
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn tags_in(tokens: &[String]) -> Vec<&'static str> {
    TAGS.iter()
        .filter(|(_, aliases)| tokens.iter().any(|token| aliases.contains(&token.as_str())))
        .map(|(tag, _)| *tag)
        .collect()
}

impl TrackSignature {
    pub fn embedded(language: Option<&str>, title: Option<&str>) -> Self {
        let words = title.map(tokens).unwrap_or_default();
        let language = language
            .and_then(normalize_language)
            .or_else(|| words.iter().find_map(|word| normalize_language(word)));
        Self {
            language,
            tags: tags_in(&words),
            title: title
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_lowercase),
            external: false,
            ordinal: 0,
        }
    }

    /// `file_stem` is the subtitle name without extension; `media_stem` the playing file's.
    pub fn external(file_stem: &str, media_stem: Option<&str>) -> Self {
        let descriptor = media_stem
            .and_then(|media| file_stem.strip_prefix(media))
            .map(tokens);
        let words = descriptor.unwrap_or_else(|| {
            let all = tokens(file_stem);
            all[all.len().saturating_sub(TRAILING_TOKENS)..].to_vec()
        });
        Self {
            language: words.iter().rev().find_map(|word| normalize_language(word)),
            tags: tags_in(&words),
            title: None,
            external: true,
            ordinal: 0,
        }
    }

    fn same_group(&self, other: &Self) -> bool {
        self.external == other.external && self.language == other.language
    }

    /// Numbers each signature within its kind and language, in order.
    pub fn assign_ordinals(signatures: &mut [Self]) {
        for index in 0..signatures.len() {
            let (before, rest) = signatures.split_at_mut(index);
            let current = &mut rest[0];
            current.ordinal = before.iter().filter(|s| s.same_group(current)).count();
        }
    }

    /// How well `candidate` stands in for `self`; `None` when it is a different track.
    pub fn affinity(&self, candidate: &Self) -> Option<i32> {
        if self.language != candidate.language {
            return None;
        }
        let same_title = self.title.is_some() && self.title == candidate.title;
        let same_slot = self.external == candidate.external && self.ordinal == candidate.ordinal;
        if self.language.is_none() && !same_title && !same_slot {
            return None;
        }
        let shared = self
            .tags
            .iter()
            .filter(|t| candidate.tags.contains(t))
            .count();
        let differing = self.tags.len() + candidate.tags.len() - 2 * shared;
        let forced_mismatch = self.tags.contains(&"forced") != candidate.tags.contains(&"forced");
        let mut score = 10 + 2 * shared as i32 - 2 * differing as i32;
        score -= if forced_mismatch { 6 } else { 0 };
        score += if same_title { 4 } else { 0 };
        score += i32::from(self.external == candidate.external);
        score += i32::from(same_slot);
        Some(score)
    }

    /// Index of the best stand-in, the earliest one on ties.
    pub fn best_match(&self, candidates: &[Self]) -> Option<usize> {
        candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| Some((index, self.affinity(candidate)?)))
            .fold(
                None,
                |best: Option<(usize, i32)>, (index, score)| match best {
                    Some((_, top)) if top >= score => best,
                    _ => Some((index, score)),
                },
            )
            .map(|(index, _)| index)
    }
}
