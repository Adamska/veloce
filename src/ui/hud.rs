use egui::{Align, Align2, Area, Color32, Id, Layout, Order, Rect, RichText, Ui};

use super::{
    ViewContext,
    action::Action,
    filters_panel::Tab,
    icons::Icon,
    scrubber::{self, ScrubberModel},
    theme,
    widgets::{capsule, format_timecode, icon_button},
};
use crate::player::{Command, command::format_speed};

const MAX_WIDTH: f32 = 940.0;

pub struct Toggles {
    pub playlist: bool,
    pub filters: bool,
    pub inspector: bool,
}

fn transport(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
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
    ui.label(
        RichText::new(format_timecode(player.position()))
            .font(theme::mono(12.5))
            .color(theme::TEXT),
    );
    let total = player
        .duration()
        .map_or_else(|| "LIVE".to_owned(), format_timecode);
    ui.label(
        RichText::new(format!("/ {total}"))
            .font(theme::mono(12.5))
            .color(theme::TEXT_DIM),
    );
}

fn speed_stepper(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
    if icon_button(ui, Icon::Plus, "Faster (])", false).clicked() {
        actions.push(Command::StepSpeed { faster: true }.into());
    }
    let effective = view.player.effective_speed();
    let boosted = view.player.is_skipping_silence();
    let shown = if boosted {
        effective
    } else {
        view.player.speed()
    };
    let color = if boosted {
        theme::WARNING
    } else if (shown - 1.0).abs() > 1e-3 {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    let label = RichText::new(format_speed(shown))
        .font(theme::mono(12.0))
        .color(color);
    if ui
        .add(
            egui::Button::new(label)
                .frame(false)
                .min_size([46.0, 22.0].into()),
        )
        .on_hover_text("Reset speed (=)")
        .clicked()
    {
        actions.push(Command::ResetSpeed.into());
    }
    if icon_button(ui, Icon::Minus, "Slower ([)", false).clicked() {
        actions.push(Command::StepSpeed { faster: false }.into());
    }
}

fn tools(ui: &mut Ui, view: &ViewContext<'_>, toggles: &Toggles, actions: &mut Vec<Action>) {
    let player = view.player;
    if icon_button(ui, Icon::Fullscreen, "Fullscreen (F)", false).clicked() {
        actions.push(Action::ToggleFullscreen);
    }
    if icon_button(ui, Icon::Info, "Inspector (Cmd/Ctrl+I)", toggles.inspector).clicked() {
        actions.push(Action::ToggleInspector);
    }
    if icon_button(
        ui,
        Icon::Playlist,
        "Playlist (Cmd/Ctrl+L)",
        toggles.playlist,
    )
    .clicked()
    {
        actions.push(Action::TogglePlaylist);
    }
    if icon_button(
        ui,
        Icon::Sliders,
        "Filters and equalizer (E)",
        toggles.filters,
    )
    .clicked()
    {
        actions.push(Action::ToggleFilters);
    }
    let looping = player.loop_points().0.is_some();
    if icon_button(ui, Icon::Loop, "A-B loop (L)", looping).clicked() {
        actions.push(Command::ToggleLoopPoint.into());
    }
    let audio = icon_button(
        ui,
        Icon::AudioTrack,
        "Cycle audio track (B), right-click for options",
        false,
    );
    if audio.clicked() {
        actions.push(Command::CycleAudioTrack.into());
    } else if audio.secondary_clicked() {
        actions.push(Action::ShowFilters(Tab::Audio));
    }
    let subtitled = player.subtitles.selection.is_some();
    let subtitles = icon_button(
        ui,
        Icon::Subtitles,
        "Cycle subtitles (S), right-click for options",
        subtitled,
    );
    if subtitles.clicked() {
        actions.push(Command::CycleSubtitles.into());
    } else if subtitles.secondary_clicked() {
        actions.push(Action::ShowFilters(Tab::Subtitles));
    }
    ui.add_space(4.0);
    let mut volume = player.volume();
    ui.spacing_mut().slider_width = 78.0;
    let slider = ui
        .add(egui::Slider::new(&mut volume, 0.0..=2.0).show_value(false))
        .on_hover_text(format!("Volume {:.0}%", volume * 100.0));
    if slider.changed() {
        actions.push(Command::SetVolume(volume).into());
    }
    let icon = if player.is_muted() || player.volume() <= 0.0 {
        Icon::Muted
    } else {
        Icon::Volume
    };
    if icon_button(ui, icon, "Mute (M)", false).clicked() {
        actions.push(Command::ToggleMute.into());
    }
    ui.add_space(4.0);
    speed_stepper(ui, view, actions);
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
    let screen = ctx.content_rect();
    let width = (screen.width() - 48.0).min(MAX_WIDTH);
    let response = Area::new(Id::unique("hud"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_BOTTOM, [0.0, -22.0])
        .interactable(opacity > 0.4)
        .show(ctx, |ui| {
            ui.multiply_opacity(opacity);
            capsule(1.0).show(ui, |ui| {
                ui.set_width(width - 28.0);
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
                    transport(ui, view, actions);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        tools(ui, view, toggles, actions);
                    });
                });
            });
        });

    if let Some(info) = player.info() {
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::unique("title")));
        let origin = screen.left_top() + egui::vec2(24.0, 40.0);
        let chapter = info
            .chapter_at(player.position())
            .map(|chapter| chapter.title.as_str());
        let lines = [
            (
                Some(info.title.as_str()),
                0.0,
                theme::sans(15.0),
                theme::TEXT,
            ),
            (chapter, 22.0, theme::sans(12.0), theme::TEXT_DIM),
        ];
        for (text, top, font, color) in lines {
            let Some(text) = text else {
                continue;
            };
            for (offset, color) in [(1.0, Color32::from_black_alpha(200)), (0.0, color)] {
                painter.text(
                    origin + egui::vec2(0.0, top + offset),
                    Align2::LEFT_TOP,
                    text,
                    font.clone(),
                    theme::fade(color, opacity),
                );
            }
        }
    }
    Some(response.response.rect)
}
