use crate::media::Micros;

const TAIL_ALLOWANCE: Micros = 30_000_000;

#[derive(Clone, Debug, Default)]
pub struct KeyframeIndex {
    times: Vec<Micros>,
    complete: bool,
}

impl KeyframeIndex {
    pub fn from_container(mut times: Vec<Micros>, duration: Option<Micros>) -> Self {
        times.sort_unstable();
        times.dedup();
        let covers_stream = match (times.last(), duration) {
            (Some(last), Some(duration)) => *last >= duration - duration / 5 - TAIL_ALLOWANCE,
            _ => false,
        };
        Self {
            complete: times.len() > 1 && covers_stream,
            times,
        }
    }

    pub fn len(&self) -> usize {
        self.times.len()
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn preceding(&self, target: Micros) -> Option<Micros> {
        let count = self.times.partition_point(|time| *time <= target);
        count.checked_sub(1).map(|index| self.times[index])
    }

    pub fn following(&self, target: Micros) -> Option<Micros> {
        let index = self.times.partition_point(|time| *time <= target);
        self.times.get(index).copied()
    }

    pub fn seek_anchor(&self, target: Micros) -> Micros {
        if self.complete {
            self.preceding(target).unwrap_or(target)
        } else {
            target
        }
    }

    pub fn reachable_by_decoding_forward(&self, position: Micros, target: Micros) -> bool {
        self.complete
            && target >= position
            && self
                .preceding(target)
                .is_some_and(|keyframe| keyframe <= position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> KeyframeIndex {
        let times = vec![4_000_000, 0, 2_000_000, 2_000_000, 6_000_000];
        KeyframeIndex::from_container(times, Some(8_000_000))
    }

    #[test]
    fn container_entries_are_sorted_and_deduplicated() {
        assert_eq!(index().len(), 4);
        assert!(index().is_complete());
    }

    #[test]
    fn preceding_returns_the_keyframe_at_or_before_the_target() {
        let index = index();
        assert_eq!(index.preceding(2_000_000), Some(2_000_000));
        assert_eq!(index.preceding(3_999_999), Some(2_000_000));
        assert_eq!(index.preceding(-1), None);
        assert_eq!(index.following(2_000_000), Some(4_000_000));
        assert_eq!(index.following(6_000_000), None);
    }

    #[test]
    fn partial_indexes_are_never_trusted_for_seek_anchors() {
        let observed = vec![0, 2_000_000, 4_000_000, 6_000_000];
        let partial = KeyframeIndex::from_container(observed, Some(600_000_000));
        assert!(!partial.is_complete());
        assert_eq!(partial.seek_anchor(90_000_000), 90_000_000);
        assert!(!partial.reachable_by_decoding_forward(6_500_000, 7_000_000));
        let live = KeyframeIndex::from_container(vec![0, 2_000_000], None);
        assert!(!live.is_complete());
        assert_eq!(index().seek_anchor(5_000_000), 4_000_000);
    }

    #[test]
    fn forward_roll_is_only_chosen_inside_the_current_gop() {
        let index = index();
        assert!(index.reachable_by_decoding_forward(2_500_000, 3_900_000));
        assert!(!index.reachable_by_decoding_forward(2_500_000, 4_100_000));
        assert!(!index.reachable_by_decoding_forward(2_500_000, 2_100_000));
    }
}
