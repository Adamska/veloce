use super::filters_panel::Tab;
use crate::player::Command;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Player(Command),
    OpenFileDialog,
    AddFilesDialog,
    OpenSubtitleDialog,
    ShowNetworkDialog,
    TogglePlaylist,
    ToggleFilters,
    ShowFilters(Tab),
    ToggleInspector,
    ToggleFullscreen,
    Snapshot,
    Dismiss,
    Quit,
}

impl From<Command> for Action {
    fn from(command: Command) -> Self {
        Self::Player(command)
    }
}
