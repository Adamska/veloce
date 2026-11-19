use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use super::{
    audio_timeline::AudioTimeline,
    clock::monotonic_nanos,
    frame_cache::{CacheLimits, FrameCache},
    pad::Pad,
};
use crate::media::{MICROS_PER_SECOND, Micros};

pub const SPARE_LANES: usize = 4;
pub const ROLL_LIMIT: Micros = 400_000;
pub(super) const PAD_BEHIND: Micros = 100_000;
pub(super) const PAD_AHEAD: Micros = 600_000;
const PAD_GUARANTEED: Micros = 150_000;
const PAD_BUDGET_SHARE: usize = 16;
pub(super) const AIM_SLACK: Micros = 100_000;
const WARM_NANOS: u64 = 5_000_000_000;
const MIN_STRIDE: Micros = 500_000;
pub(super) const UNSET: i64 = i64::MIN;
const DEFAULT_HOPS: [Micros; SPARE_LANES] = [
    5 * MICROS_PER_SECOND,
    -5 * MICROS_PER_SECOND,
    30 * MICROS_PER_SECOND,
    10 * MICROS_PER_SECOND,
];

pub fn decode_threads(_lane: usize) -> usize {
    0
}

pub fn pad_limits(limits: &CacheLimits) -> CacheLimits {
    CacheLimits {
        behind: PAD_BEHIND,
        ahead: PAD_AHEAD,
        guaranteed_ahead: PAD_GUARANTEED,
        budget_bytes: limits.budget_bytes / PAD_BUDGET_SHARE,
    }
}

pub struct Lanes {
    pads: Vec<Pad>,
    active: AtomicUsize,
    last_seek: AtomicU64,
    building: AtomicU64,
    held: AtomicBool,
    next_hop: AtomicUsize,
}

impl Lanes {
    pub fn new(spare: usize, limits: CacheLimits, sample_rate: u32) -> Self {
        Self {
            pads: (0..=spare).map(|_| Pad::new(limits, sample_rate)).collect(),
            active: AtomicUsize::new(0),
            last_seek: AtomicU64::new(0),
            building: AtomicU64::new(0),
            held: AtomicBool::new(false),
            next_hop: AtomicUsize::new(usize::MAX),
        }
    }

    pub fn count(&self) -> usize {
        self.pads.len()
    }

    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    pub fn pad(&self, lane: usize) -> &Pad {
        &self.pads[lane.min(self.pads.len() - 1)]
    }

    pub fn set_building(&self, lane: usize, building: bool) {
        let bit = 1u64 << (lane % 64);
        if building {
            self.building.fetch_or(bit, Ordering::AcqRel);
        } else {
            self.building.fetch_and(!bit, Ordering::AcqRel);
        }
    }

    pub fn any_building(&self) -> bool {
        self.building.load(Ordering::Acquire) != 0
    }

    pub fn serves_next_hop(&self, lane: usize) -> bool {
        self.next_hop.load(Ordering::Acquire) == lane
    }

    pub fn set_held(&self, held: bool) {
        self.held.store(held, Ordering::Release);
    }

    pub fn is_warm(&self) -> bool {
        let last_seek = self.last_seek.load(Ordering::Acquire);
        let recent = last_seek != 0 && monotonic_nanos().saturating_sub(last_seek) < WARM_NANOS;
        recent || self.held.load(Ordering::Acquire)
    }

    pub fn offset(&self, lane: usize) -> Option<Micros> {
        Some(self.pad(lane).offset.load(Ordering::Acquire)).filter(|offset| *offset != UNSET)
    }

    pub(super) fn touch(&self) {
        self.last_seek
            .store(monotonic_nanos().max(1), Ordering::Release);
    }

    pub(super) fn invalidate(&self) {
        for pad in &self.pads {
            pad.stale.store(true, Ordering::Release);
        }
    }

    pub(super) fn plan(&self, target: Micros, stride: Option<Micros>, video: bool) {
        let stride = stride.filter(|stride| stride.abs() >= MIN_STRIDE);
        let hops = match stride {
            Some(stride) => [stride, 2 * stride, -stride, 3 * stride],
            None => DEFAULT_HOPS,
        };
        let active = self.active();
        let spare: Vec<usize> = (0..self.pads.len())
            .filter(|lane| *lane != active)
            .collect();
        let mut assigned = vec![UNSET; self.pads.len()];
        let mut open: Vec<Micros> = hops.into_iter().take(spare.len()).collect();
        for lane in &spare {
            let serving = open
                .iter()
                .position(|hop| self.pads[*lane].serves(target + hop, video));
            if let Some(index) = serving {
                assigned[*lane] = open.remove(index);
            }
        }
        for lane in spare {
            if assigned[lane] == UNSET && !open.is_empty() {
                assigned[lane] = open.remove(0);
            }
        }
        let next_hop = stride
            .and_then(|stride| assigned.iter().position(|offset| *offset == stride))
            .unwrap_or(usize::MAX);
        self.next_hop.store(next_hop, Ordering::Release);
        for (pad, offset) in self.pads.iter().zip(assigned) {
            pad.offset.store(offset, Ordering::Release);
        }
    }

    pub(super) fn promote(
        &self,
        frames: &FrameCache,
        audio: &AudioTimeline,
        target: Micros,
        video: bool,
    ) -> Option<bool> {
        let active = self.active();
        let candidates = || (0..self.pads.len()).filter(|lane| *lane != active);
        let (lane, decoded) = candidates()
            .find(|lane| self.pads[*lane].covers(target, video))
            .map(|lane| (lane, true))
            .or_else(|| {
                candidates()
                    .find(|lane| self.pads[*lane].aims_at(target))
                    .map(|lane| (lane, false))
            })?;
        let (promoted, retired) = (&self.pads[lane], &self.pads[active]);
        frames.exchange(&promoted.frames);
        audio.exchange(&promoted.audio);
        promoted.frames.exchange(&retired.frames);
        promoted.audio.exchange(&retired.audio);
        promoted.set_aim(None);
        retired.set_aim(None);
        self.active.store(lane, Ordering::Release);
        Some(decoded)
    }
}
