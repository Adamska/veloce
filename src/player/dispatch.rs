use std::sync::atomic::Ordering;

use super::{
    Command, DEMO_LOCATION, Player,
    command::{self, SPEED_STEPS, SubtitleSelection},
};
use crate::{
    audio::MAX_VOLUME,
    media::{MICROS_PER_SECOND, Micros},
};

const MAX_DELAY: Micros = 5 * MICROS_PER_SECOND;

impl Player {
    fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(SPEED_STEPS[0], SPEED_STEPS[SPEED_STEPS.len() - 1]);
        self.controls().set_speed(self.speed);
        self.notify(format!("Speed {}", command::format_speed(self.speed)));
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, MAX_VOLUME);
        self.controls().set_volume(self.volume);
        if self.muted && self.volume > 0.0 {
            self.muted = false;
            self.controls().muted.store(false, Ordering::Relaxed);
        }
    }

    fn select_audio(&mut self, track: Option<usize>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let label = match (track, session.info.as_ref()) {
            (Some(index), Some(info)) => match info.audio_tracks.get(index) {
                Some(track) => track.label(index),
                None => return,
            },
            _ => "Off".to_owned(),
        };
        session.shared.set_audio_track(track);
        self.resynchronize(true);
        self.notify(format!("Audio track: {label}"));
    }

    fn cycle_audio(&mut self) {
        let count = self.info().map_or(0, |info| info.audio_tracks.len());
        if count == 0 {
            return;
        }
        let next = match self.audio_track() {
            Some(index) if index + 1 < count => Some(index + 1),
            Some(_) => None,
            None => Some(0),
        };
        self.select_audio(next);
    }

    pub fn execute(&mut self, command: Command) {
        match command {
            Command::TogglePause if self.ended => {
                self.seek_to(0);
                self.set_paused(false);
            }
            Command::TogglePause if self.session.is_some() => self.set_paused(!self.paused),
            Command::TogglePause => {}
            Command::SeekBy(delta) => self.seek_by(delta),
            Command::SeekTo(target) => self.seek_to(target),
            Command::StepFrame { forward } => self.step_frame(forward),
            Command::SetSpeed(speed) => self.set_speed(speed),
            Command::StepSpeed { faster } => {
                self.set_speed(command::step_speed(self.speed, faster))
            }
            Command::ResetSpeed => self.set_speed(1.0),
            Command::SetVolume(volume) => self.set_volume(volume),
            Command::AdjustVolume(delta) => {
                self.set_volume(self.volume + delta);
                self.notify(format!("Volume {:.0}%", self.volume * 100.0));
            }
            Command::ToggleMute => {
                self.muted = !self.muted;
                self.controls().muted.store(self.muted, Ordering::Relaxed);
                self.notify(if self.muted { "Muted" } else { "Unmuted" });
            }
            Command::CycleAudioTrack => self.cycle_audio(),
            Command::SelectAudioTrack(track) => self.select_audio(track),
            Command::CycleSubtitles => {
                self.subtitles.cycle();
                self.notify(format!("Subtitles: {}", self.subtitles.label()));
            }
            Command::SelectSubtitles(selection) => self.select_subtitles(selection),
            Command::AdjustAudioDelay(delta) => {
                self.audio_delay = (self.audio_delay + delta).clamp(-MAX_DELAY, MAX_DELAY);
                self.controls()
                    .delay_micros
                    .store(self.audio_delay, Ordering::Relaxed);
                self.resynchronize(false);
                self.notify(format!(
                    "Audio delay {}",
                    command::format_delay(self.audio_delay)
                ));
            }
            Command::AdjustSubtitleDelay(delta) => {
                self.subtitles.delay = (self.subtitles.delay + delta).clamp(-MAX_DELAY, MAX_DELAY);
                let delay = command::format_delay(self.subtitles.delay);
                self.notify(format!("Subtitle delay {delay}"));
            }
            Command::ToggleLoopPoint => self.toggle_loop_point(),
            Command::SkipChapter { forward } => self.skip_chapter(forward),
            Command::SetAdjustments(adjustments) => self.adjustments = adjustments,
            Command::SetGeometry(geometry) => self.geometry = geometry,
            Command::SetSubtitleStyle(style) => self.subtitles.style = style,
            Command::Open(locations) if !locations.is_empty() => {
                self.playlist.replace(locations);
                self.start(0);
            }
            Command::Open(_) => {}
            Command::Enqueue(locations) => {
                let first_new = self.playlist.items().len();
                self.playlist.extend(locations);
                if self.session.is_none() {
                    self.start(first_new);
                }
            }
            Command::OpenDemo => self.execute(Command::Open(vec![DEMO_LOCATION.to_owned()])),
            Command::LoadSubtitles(path) => match self.subtitles.load_external(&path) {
                Ok(name) => self.notify(format!("Subtitles: {name}")),
                Err(error) => self.report_error(format!("{error:#}")),
            },
            Command::PlayIndex(index) => self.start(index),
            Command::RemoveIndex(index) => {
                let playing = self.playlist.current() == Some(index);
                self.playlist.remove(index);
                if playing {
                    self.close();
                }
            }
            Command::PlayNext => {
                if let Some(index) = self.playlist.advance(false) {
                    self.start(index);
                }
            }
            Command::PlayPrevious => {
                if let Some(index) = self.playlist.retreat() {
                    self.start(index);
                }
            }
            Command::ToggleShuffle => {
                let shuffle = !self.playlist.shuffle;
                self.playlist.set_shuffle(shuffle);
                self.notify(if shuffle { "Shuffle on" } else { "Shuffle off" });
            }
            Command::CycleRepeat => {
                self.playlist.repeat = self.playlist.repeat.next();
                self.notify(self.playlist.repeat.label());
            }
            Command::Stop => self.close(),
        }
    }

    fn select_subtitles(&mut self, selection: Option<SubtitleSelection>) {
        self.subtitles.selection = selection;
        self.notify(format!("Subtitles: {}", self.subtitles.label()));
    }
}
