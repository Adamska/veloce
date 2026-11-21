use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::{
    player::{
        Command,
        command::{
            AUDIO_DELAY_STEP, SEEK_LONG, SEEK_SHORT, SPEED_FINE_STEP, SUBTITLE_DELAY_STEP,
            VOLUME_STEP,
        },
    },
    ui::Action,
};

pub fn primary_modifier(modifiers: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.super_key()
    } else {
        modifiers.control_key()
    }
}

fn named(key: NamedKey, modifiers: ModifiersState, repeat: bool) -> Option<Action> {
    let seek = if modifiers.shift_key() {
        SEEK_LONG
    } else {
        SEEK_SHORT
    };
    let command = match key {
        NamedKey::Space if !repeat => Command::TogglePause,
        NamedKey::ArrowLeft => Command::SeekBy(-seek),
        NamedKey::ArrowRight => Command::SeekBy(seek),
        NamedKey::ArrowUp => Command::AdjustVolume(VOLUME_STEP),
        NamedKey::ArrowDown => Command::AdjustVolume(-VOLUME_STEP),
        NamedKey::Escape if !repeat => return Some(Action::Dismiss),
        NamedKey::MediaPlayPause if !repeat => Command::TogglePause,
        NamedKey::MediaTrackNext if !repeat => Command::PlayNext,
        NamedKey::MediaTrackPrevious if !repeat => Command::PlayPrevious,
        _ => return None,
    };
    Some(command.into())
}

fn with_primary(character: &str) -> Option<Action> {
    Some(match character {
        "i" => Action::ToggleInspector,
        "s" => Action::Snapshot,
        "u" => Action::ShowNetworkDialog,
        "o" => Action::OpenFileDialog,
        "l" => Action::TogglePlaylist,
        "e" => Action::ToggleFilters,
        "q" => Action::Quit,
        _ => return None,
    })
}

fn character(character: &str, modifiers: ModifiersState, repeat: bool) -> Option<Action> {
    let repeatable = match character {
        "." => Some(Command::StepFrame { forward: true }),
        "," => Some(Command::StepFrame { forward: false }),
        "]" => Some(Command::StepSpeed { faster: true }),
        "[" => Some(Command::StepSpeed { faster: false }),
        "}" => Some(Command::AdjustSpeed(SPEED_FINE_STEP)),
        "{" => Some(Command::AdjustSpeed(-SPEED_FINE_STEP)),
        "j" => Some(Command::AdjustAudioDelay(-AUDIO_DELAY_STEP)),
        "k" => Some(Command::AdjustAudioDelay(AUDIO_DELAY_STEP)),
        "g" => Some(Command::AdjustSubtitleDelay(-SUBTITLE_DELAY_STEP)),
        "h" => Some(Command::AdjustSubtitleDelay(SUBTITLE_DELAY_STEP)),
        _ => None,
    };
    if let Some(command) = repeatable {
        return Some(command.into());
    }
    if repeat {
        return None;
    }
    let shift = modifiers.shift_key();
    Some(match character {
        "=" => Command::ResetSpeed.into(),
        "f" => Action::ToggleFullscreen,
        "m" => Command::ToggleMute.into(),
        "s" => Command::CycleSubtitles.into(),
        "b" => Command::CycleAudioTrack.into(),
        "l" => Command::ToggleLoopPoint.into(),
        "e" => Action::ToggleFilters,
        "n" if shift => Command::SkipChapter { forward: true }.into(),
        "p" if shift => Command::SkipChapter { forward: false }.into(),
        "n" => Command::PlayNext.into(),
        "p" => Command::PlayPrevious.into(),
        _ => return None,
    })
}

pub fn map(key: &Key, modifiers: ModifiersState, repeat: bool) -> Option<Action> {
    match key {
        Key::Named(key) => named(*key, modifiers, repeat),
        Key::Character(text) => {
            let lowered = text.to_lowercase();
            if primary_modifier(modifiers) {
                (!repeat).then(|| with_primary(&lowered)).flatten()
            } else if modifiers.alt_key() || modifiers.control_key() || modifiers.super_key() {
                None
            } else {
                character(&lowered, modifiers, repeat)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &str, modifiers: ModifiersState) -> Option<Action> {
        map(&Key::Character(text.into()), modifiers, false)
    }

    fn primary() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        }
    }

    #[test]
    fn arrows_seek_five_seconds_and_thirty_with_shift() {
        let none = ModifiersState::empty();
        let right = map(&Key::Named(NamedKey::ArrowRight), none, true);
        assert_eq!(right, Some(Command::SeekBy(5_000_000).into()));
        let left = map(
            &Key::Named(NamedKey::ArrowLeft),
            ModifiersState::SHIFT,
            false,
        );
        assert_eq!(left, Some(Command::SeekBy(-30_000_000).into()));
    }

    #[test]
    fn transport_keys_follow_the_vlc_map() {
        let none = ModifiersState::empty();
        assert_eq!(
            map(&Key::Named(NamedKey::Space), none, false),
            Some(Command::TogglePause.into())
        );
        assert_eq!(map(&Key::Named(NamedKey::Space), none, true), None);
        assert_eq!(
            press(".", none),
            Some(Command::StepFrame { forward: true }.into())
        );
        assert_eq!(
            press("[", none),
            Some(Command::StepSpeed { faster: false }.into())
        );
        assert_eq!(press("=", none), Some(Command::ResetSpeed.into()));
        assert_eq!(
            press("}", ModifiersState::SHIFT),
            Some(Command::AdjustSpeed(0.05).into())
        );
        assert_eq!(press("F", none), Some(Action::ToggleFullscreen));
        assert_eq!(press("m", none), Some(Command::ToggleMute.into()));
        assert_eq!(press("s", none), Some(Command::CycleSubtitles.into()));
        assert_eq!(press("b", none), Some(Command::CycleAudioTrack.into()));
        assert_eq!(press("l", none), Some(Command::ToggleLoopPoint.into()));
    }

    #[test]
    fn sync_keys_use_the_documented_increments() {
        let none = ModifiersState::empty();
        assert_eq!(
            press("j", none),
            Some(Command::AdjustAudioDelay(-25_000).into())
        );
        assert_eq!(
            press("k", none),
            Some(Command::AdjustAudioDelay(25_000).into())
        );
        assert_eq!(
            press("g", none),
            Some(Command::AdjustSubtitleDelay(-50_000).into())
        );
        assert_eq!(
            press("h", none),
            Some(Command::AdjustSubtitleDelay(50_000).into())
        );
    }

    #[test]
    fn primary_modifier_selects_application_shortcuts() {
        assert_eq!(press("i", primary()), Some(Action::ToggleInspector));
        assert_eq!(press("s", primary()), Some(Action::Snapshot));
        assert_eq!(press("u", primary()), Some(Action::ShowNetworkDialog));
        assert_eq!(press("x", primary()), None);
        assert_eq!(press("i", ModifiersState::empty()), None);
    }
}
