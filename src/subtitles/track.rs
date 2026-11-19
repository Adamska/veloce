use super::parser::Cue;
use crate::media::Micros;

#[derive(Clone, Debug, Default)]
pub struct SubtitleTrack {
    cues: Vec<Cue>,
    longest: Micros,
}

impl SubtitleTrack {
    pub fn from_cues(mut cues: Vec<Cue>) -> Self {
        cues.sort_by_key(|cue| (cue.start, cue.end));
        cues.dedup();
        let longest = cues
            .iter()
            .map(|cue| cue.end - cue.start)
            .max()
            .unwrap_or(0);
        Self { cues, longest }
    }

    pub fn len(&self) -> usize {
        self.cues.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    pub fn insert(&mut self, cue: Cue) -> bool {
        if cue.end <= cue.start || cue.text.is_empty() {
            return false;
        }
        let slot = self
            .cues
            .partition_point(|existing| existing.start < cue.start);
        let duplicate = self.cues[slot..]
            .iter()
            .take_while(|existing| existing.start == cue.start)
            .any(|existing| *existing == cue);
        if duplicate {
            return false;
        }
        self.longest = self.longest.max(cue.end - cue.start);
        self.cues.insert(slot, cue);
        true
    }

    pub fn active_at(&self, time: Micros) -> impl Iterator<Item = &Cue> {
        let first = self
            .cues
            .partition_point(|cue| cue.start < time - self.longest);
        let last = self.cues.partition_point(|cue| cue.start <= time);
        self.cues[first..last]
            .iter()
            .filter(move |cue| cue.end > time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: Micros, end: Micros, text: &str) -> Cue {
        Cue {
            start,
            end,
            text: text.to_owned(),
        }
    }

    fn texts(track: &SubtitleTrack, time: Micros) -> Vec<&str> {
        track.active_at(time).map(|cue| cue.text.as_str()).collect()
    }

    #[test]
    fn finds_cues_active_at_a_timestamp() {
        let track = SubtitleTrack::from_cues(vec![
            cue(5_000, 9_000, "second"),
            cue(1_000, 3_000, "first"),
            cue(8_000, 12_000, "third"),
        ]);
        assert_eq!(texts(&track, 500), Vec::<&str>::new());
        assert_eq!(texts(&track, 1_000), vec!["first"]);
        assert_eq!(texts(&track, 3_000), Vec::<&str>::new());
        assert_eq!(texts(&track, 8_500), vec!["second", "third"]);
    }

    #[test]
    fn long_cues_remain_visible_behind_shorter_ones() {
        let mut track = SubtitleTrack::default();
        assert!(track.insert(cue(0, 60_000_000, "sign")));
        for index in 0..50 {
            assert!(track.insert(cue(index * 1_000_000, index * 1_000_000 + 900_000, "line")));
        }
        assert_eq!(texts(&track, 49_500_000), vec!["sign", "line"]);
    }

    #[test]
    fn repeated_decoding_after_a_seek_does_not_duplicate_cues() {
        let mut track = SubtitleTrack::default();
        assert!(track.insert(cue(2_000, 4_000, "hello")));
        assert!(!track.insert(cue(2_000, 4_000, "hello")));
        assert!(track.insert(cue(2_000, 4_000, "other")));
        assert!(!track.insert(cue(3_000, 3_000, "empty span")));
        assert_eq!(track.len(), 2);
    }
}
