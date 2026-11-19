use std::{path::Path, sync::atomic::Ordering, time::Instant};

use super::{DEMO_LOCATION, Player, session::Session};
use crate::{
    engine::demuxer::is_network_location,
    media::{MICROS_PER_SECOND, Micros},
};

const MIN_LOOP: Micros = 100_000;
const CHAPTER_RESTART_WINDOW: Micros = 3 * MICROS_PER_SECOND;

impl Player {
    pub(super) fn close(&mut self) {
        self.audio.attach(None);
        self.session = None;
        self.set_paused(true);
        self.position = 0;
        self.loop_points = (None, None);
        self.presented = None;
        self.seek_probe = None;
    }

    pub(super) fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.controls().paused.store(paused, Ordering::Release);
        if let Some(session) = self.session.as_ref() {
            session.shared.lanes().set_held(paused);
        }
    }

    pub(super) fn start(&mut self, index: usize) {
        let Some(item) = self.playlist.select(index).cloned() else {
            return;
        };
        self.close();
        self.subtitles.reset();
        let opened = Session::open(
            &item.location,
            self.audio.sample_rate(),
            self.capabilities,
            self.cache_limits,
        );
        match opened {
            Ok(session) => {
                self.clock.hold(0, session.shared.epoch());
                self.audio.attach(Some(session.shared.clone()));
                self.session = Some(session);
                self.ended = false;
                self.set_paused(false);
                if item.location != DEMO_LOCATION && !is_network_location(&item.location) {
                    self.subtitles.load_sidecars(Path::new(&item.location));
                }
            }
            Err(error) => self.report_error(format!("Cannot open {}: {error:#}", item.title)),
        }
    }

    pub(super) fn finish(&mut self) {
        match self.playlist.advance(true) {
            Some(index) => self.start(index),
            None => {
                self.ended = true;
                self.set_paused(true);
            }
        }
    }

    pub(super) fn seek_to(&mut self, target: Micros) {
        self.seek_with_stride(target, None);
    }

    pub(super) fn seek_by(&mut self, delta: Micros) {
        self.seek_with_stride(self.position + delta, Some(delta));
    }

    fn seek_with_stride(&mut self, target: Micros, stride: Option<Micros>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let limit = session
            .info
            .as_ref()
            .and_then(|info| info.duration)
            .map_or(Micros::MAX, |duration| (duration - 50_000).max(0));
        let target = target.clamp(0, limit);
        let seekable = session.info.as_ref().is_none_or(|info| info.seekable);
        if !seekable && !session.shared.cache.covers(target) {
            self.notify("This stream can only seek inside the buffered window");
            return;
        }
        let (_, epoch) = match stride {
            Some(stride) => session.shared.hop(target, stride),
            None => session.shared.seek(target),
        };
        self.clock.hold(target, epoch);
        self.position = target;
        self.seek_probe = Some((epoch, Instant::now()));
        self.ended = false;
    }

    pub(super) fn resynchronize(&mut self, reload: bool) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let epoch = if reload {
            session.shared.reload(self.position)
        } else {
            session.shared.seek(self.position).1
        };
        self.clock.hold(self.position, epoch);
    }

    pub(super) fn step_frame(&mut self, forward: bool) {
        let Some(shared) = self.session.as_ref().map(|session| session.shared.clone()) else {
            return;
        };
        self.set_paused(true);
        let cache = &shared.cache;
        let current = cache.frame_at(self.position);
        let reference = current.as_ref().map_or(self.position, |frame| frame.pts);
        let nominal = current
            .as_ref()
            .map_or(40_000, |frame| frame.duration.max(1));
        let target = match cache.neighbor(reference, forward) {
            Some(frame) => frame.pts,
            None if forward => reference + nominal,
            None => reference - nominal,
        };
        self.seek_to(target);
    }

    pub(super) fn toggle_loop_point(&mut self) {
        self.loop_points = match self.loop_points {
            (None, _) => (Some(self.position), None),
            (Some(start), None) if self.position > start + MIN_LOOP => {
                (Some(start), Some(self.position))
            }
            (Some(start), None) => (Some(start), None),
            (Some(_), Some(_)) => (None, None),
        };
        self.notify(match self.loop_points {
            (Some(_), None) => "Loop: point A set",
            (Some(_), Some(_)) => "Loop: A-B active",
            _ => "Loop cleared",
        });
    }

    pub(super) fn skip_chapter(&mut self, forward: bool) {
        let Some(info) = self.info() else {
            return;
        };
        let position = self.position;
        let target = if forward {
            info.chapters
                .iter()
                .map(|chapter| chapter.start)
                .find(|start| *start > position)
        } else {
            info.chapters
                .iter()
                .rev()
                .map(|chapter| chapter.start)
                .find(|start| *start + CHAPTER_RESTART_WINDOW < position)
                .or(Some(0))
        };
        if let Some(target) = target {
            self.seek_to(target);
        }
    }
}
