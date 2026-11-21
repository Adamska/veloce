use egui::{Align, Align2, Area, Id, Layout, Order, Rect, RichText, Ui};

use super::{
    ViewContext,
    action::Action,
    filters_panel::Tab,
    icons::Icon,
    scrubber::{self, ScrubberModel},
    speed_control, theme,
    widgets::{capsule, format_timecode, icon_button},
};
use crate::player::Command;

const MAX_WIDTH: f32 = 940.0;
const COMPACT_BELOW: f32 = 850.0;

pub struct Toggles {
    pub playlist: bool,
    pub filters: bool,
    pub subtitles: bool,
    pub inspector: bool,
    pub more: bool,
}

fn transport(ui: &mut Ui, view: &ViewContext<'_>, compact: bool, actions: &mut Vec<Action>) {
    let player = view.player;
    if icon_button(ui, Icon::Previous, "Previous (P)", false).clicked() {
        actions.push(Command::PlayPrevious.into());
    }
    let (icon, hint) = if player.is_paused() {
        (Icon::Play, "Play (Space)")
    } else {
        (Icon::Pause, "Pause (Space)")
    };
    if icon_button(ui, icon, hint, false).clicked() {
        actions.push(Command::TogglePause.into());
    }
    if icon_button(ui, Icon::Next, "Next (N)", false).clicked() {
        actions.push(Command::PlayNext.into());
    }
    ui.add_space(6.0);
    let mono = |text: String, color| RichText::new(text).font(theme::mono(12.5)).color(color);
    ui.label(mono(format_timecode(player.position()), theme::TEXT));
    if !compact {
        let total = player
            .duration()
            .map_or_else(|| "LIVE".to_owned(), format_timecode);
        ui.label(mono(format!("/ {total}"), theme::TEXT_DIM));
    }
}

fn extra_controls(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
    let looping = view.player.loop_points().0.is_some();
    if icon_button(ui, Icon::Loop, "A-B loop (L)", looping).clicked() {
        actions.push(Command::ToggleLoopPoint.into());
    }
    let hint = "Cycle audio track (B), right-click for options";
    let audio = icon_button(ui, Icon::AudioTrack, hint, false);
    if audio.clicked() {
        actions.push(Command::CycleAudioTrack.into());
    } else if audio.secondary_clicked() {
        actions.push(Action::ShowFilters(Tab::Audio));
    }
}

fn panel_buttons(ui: &mut Ui, toggles: &Toggles, actions: &mut Vec<Action>) {
    let buttons = [
        (
            Icon::Fullscreen,
            "Fullscreen (F)",
            false,
            Action::ToggleFullscreen,
        ),
        (
            Icon::Info,
            "Inspector (Cmd/Ctrl+I)",
            toggles.inspector,
            Action::ToggleInspector,
        ),
        (
            Icon::Playlist,
            "Playlist (Cmd/Ctrl+L)",
            toggles.playlist,
            Action::TogglePlaylist,
        ),
        (
            Icon::Sliders,
            "Filters and equalizer (E)",
            toggles.filters,
            Action::ToggleFilters,
        ),
    ];
    for (icon, hint, active, action) in buttons {
        if icon_button(ui, icon, hint, active).clicked() {
            actions.push(action);
        }
    }
}

fn tools(
    ui: &mut Ui,
    view: &ViewContext<'_>,
    toggles: &Toggles,
    compact: bool,
    actions: &mut Vec<Action>,
) {
    let player = view.player;
    panel_buttons(ui, toggles, actions);
    let hidden_loop = !toggles.more && player.loop_points().0.is_some();
    let hint = if toggles.more {
        "Fewer controls"
    } else {
        "More controls"
    };
    if icon_button(ui, Icon::More, hint, toggles.more || hidden_loop).clicked() {
        actions.push(Action::ToggleMoreControls);
    }
    if toggles.more {
        extra_controls(ui, view, actions);
    }
    let subtitled = toggles.subtitles || player.subtitles.selection.is_some();
    if icon_button(ui, Icon::Subtitles, "Subtitles", subtitled).clicked() {
        actions.push(Action::ToggleSubtitles);
    }
    ui.add_space(4.0);
    if !compact {
        let mut volume = player.volume();
        ui.spacing_mut().slider_width = 78.0;
        let slider = ui
            .add(egui::Slider::new(&mut volume, 0.0..=2.0).show_value(false))
            .on_hover_text(format!("Volume {:.0}%", volume * 100.0));
        if slider.changed() {
            actions.push(Command::SetVolume(volume).into());
        }
    }
    let silent = player.is_muted() || player.volume() <= 0.0;
    let icon = if silent { Icon::Muted } else { Icon::Volume };
    if icon_button(ui, icon, "Mute (M)", false).clicked() {
        actions.push(Command::ToggleMute.into());
    }
    ui.add_space(4.0);
    speed_control::stepper(ui, view, actions);
}

pub fn show(
    ctx: &egui::Context,
    view: &ViewContext<'_>,
    opacity: f32,
    toggles: &Toggles,
    actions: &mut Vec<Action>,
) -> Option<Rect> {
    if opacity <= 0.001 {
        return None;
    }
    let player = view.player;
    let content_width = (ctx.content_rect().width() - 48.0).min(MAX_WIDTH) - 28.0;
    let compact = content_width < COMPACT_BELOW;
    let response = Area::new(Id::unique("hud"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_BOTTOM, [0.0, -22.0])
        .interactable(opacity > 0.4)
        .show(ctx, |ui| {
            ui.multiply_opacity(opacity);
            capsule(1.0).show(ui, |ui| {
                ui.set_width(content_width);
                let model = ScrubberModel {
                    position: player.position(),
                    duration: player.duration().unwrap_or(0),
                    buffered: player.cache_span(),
                    chapters: player.info().map_or(&[][..], |info| &info.chapters),
                    loop_points: player.loop_points(),
                    opacity,
                };
                if let Some(target) = scrubber::show(ui, &model)
                    && player.duration().is_some()
                {
                    actions.push(Command::SeekTo(target).into());
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    transport(ui, view, compact, actions);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        tools(ui, view, toggles, compact, actions);
                    });
                });
            });
        });
    Some(response.response.rect)
}
