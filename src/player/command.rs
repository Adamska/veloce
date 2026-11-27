use std::path::PathBuf;

use crate::{
    gpu::{Geometry, VideoAdjustments},
    media::Micros,
    subtitles::SubtitleStyle,
};

pub const SEEK_SHORT: Micros = 5_000_000;
pub const SEEK_LONG: Micros = 30_000_000;
pub const AUDIO_DELAY_STEP: Micros = 25_000;
pub const SUBTITLE_DELAY_STEP: Micros = 50_000;
pub const VOLUME_STEP: f32 = 0.05;
/// Increment of the fine speed control (Shift+[ and Shift+], the speed popover).
pub const SPEED_FINE_STEP: f32 = 0.05;
pub const SPEED_STEPS: [f32; 12] = [
    0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleSelection {
    Embedded(usize),
    External(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    TogglePause,
    SeekBy(Micros),
    SeekTo(Micros),
    StepFrame { forward: bool },
    SetSpeed(f32),
    StepSpeed { faster: bool },
    AdjustSpeed(f32),
    ResetSpeed,
    SetVolume(f32),
    AdjustVolume(f32),
    ToggleMute,
    CycleAudioTrack,
    SelectAudioTrack(Option<usize>),
    CycleSubtitles,
    SelectSubtitles(Option<SubtitleSelection>),
    AdjustAudioDelay(Micros),
    AdjustSubtitleDelay(Micros),
    ToggleLoopPoint,
    SkipChapter { forward: bool },
    SetAdjustments(VideoAdjustments),
    SetGeometry(Geometry),
    SetSubtitleStyle(SubtitleStyle),
    Open(Vec<String>),
    ForgetRecent(PathBuf),
    ClearRecent,
    Enqueue(Vec<String>),
    OpenDemo,
    LoadSubtitles(PathBuf),
    PlayIndex(usize),
    RemoveIndex(usize),
    PlayNext,
    PlayPrevious,
    ToggleShuffle,
    CycleRepeat,
    Stop,
}

pub fn step_speed(current: f32, faster: bool) -> f32 {
    let epsilon = 1e-3;
    let next = if faster {
        SPEED_STEPS
            .iter()
            .copied()
            .find(|step| *step > current + epsilon)
    } else {
        SPEED_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < current - epsilon)
    };
    next.unwrap_or(current)
}

/// Moves `current` by `delta` onto the grid of `SPEED_FINE_STEP`, so 1.23 + 0.05 gives 1.25.
pub fn nudge_speed(current: f32, delta: f32) -> f32 {
    let steps = ((current + delta) / SPEED_FINE_STEP).round();
    round_speed(steps * SPEED_FINE_STEP)
}

/// Rounds to hundredths, the precision shown, so repeated steps never drift to 1.2000001.
pub fn round_speed(speed: f32) -> f32 {
    (speed * 100.0).round() / 100.0
}

pub fn format_delay(delay: Micros) -> String {
    format!("{:+} ms", delay / 1_000)
}

pub fn format_speed(speed: f32) -> String {
    format!("{speed:.2}x")
}

/// A playback position as `m:ss`, or `h:mm:ss` past the hour.
pub fn format_clock(position: Micros) -> String {
    let seconds = position.max(0) / 1_000_000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_steps_walk_the_ladder_and_stop_at_the_limits() {
        assert_eq!(step_speed(1.0, true), 1.25);
        assert_eq!(step_speed(1.0, false), 0.75);
        assert_eq!(step_speed(1.1, true), 1.25);
        assert_eq!(step_speed(1.1, false), 1.0);
        assert_eq!(step_speed(4.0, true), 4.0);
        assert_eq!(step_speed(0.25, false), 0.25);
    }

    #[test]
    fn fine_speed_steps_land_on_the_twentieth_grid() {
        assert_eq!(nudge_speed(1.0, SPEED_FINE_STEP), 1.05);
        assert_eq!(nudge_speed(1.25, -SPEED_FINE_STEP), 1.2);
        assert_eq!(nudge_speed(1.23, SPEED_FINE_STEP), 1.3);
        let mut speed = 1.0;
        for _ in 0..6 {
            speed = nudge_speed(speed, SPEED_FINE_STEP);
        }
        assert_eq!(speed, 1.3);
        assert_eq!(format_speed(speed), "1.30x");
    }

    #[test]
    fn delays_and_speeds_format_for_toasts() {
        assert_eq!(format_delay(25_000), "+25 ms");
        assert_eq!(format_delay(-150_000), "-150 ms");
        assert_eq!(format_delay(0), "+0 ms");
        assert_eq!(format_speed(1.5), "1.50x");
        assert_eq!(format_clock(65_900_000), "1:05");
        assert_eq!(format_clock(3_723_000_000), "1:02:03");
        assert_eq!(format_clock(-1), "0:00");
    }
}
