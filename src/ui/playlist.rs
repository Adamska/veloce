use egui::{Color32, RichText, ScrollArea, Sense, Vec2};

use super::{
    ViewContext,
    action::Action,
    icons::{Icon, center_square},
    theme,
    widgets::{drawer, icon_button},
};
use crate::player::{Command, RepeatMode};

const WIDTH: f32 = 340.0;
const ROW_HEIGHT: f32 = 34.0;

pub fn show(ctx: &egui::Context, view: &ViewContext<'_>, openness: f32, actions: &mut Vec<Action>) {
    let playlist = &view.player.playlist;
    drawer(ctx, "playlist drawer", openness, WIDTH, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Playlist")
                    .font(theme::sans(15.0))
                    .color(theme::TEXT),
            );
            ui.label(
                RichText::new(format!("{} items", playlist.items().len()))
                    .font(theme::mono(11.0))
                    .color(theme::TEXT_FAINT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, "Close (Cmd/Ctrl+L)", false).clicked() {
                    actions.push(Action::TogglePlaylist);
                }
                let (icon, active) = match playlist.repeat {
                    RepeatMode::Off => (Icon::Repeat, false),
                    RepeatMode::All => (Icon::Repeat, true),
                    RepeatMode::One => (Icon::RepeatOne, true),
                };
                if icon_button(ui, icon, playlist.repeat.label(), active).clicked() {
                    actions.push(Command::CycleRepeat.into());
                }
                if icon_button(ui, Icon::Shuffle, "Shuffle", playlist.shuffle).clicked() {
                    actions.push(Command::ToggleShuffle.into());
                }
                if icon_button(ui, Icon::Plus, "Add files", false).clicked() {
                    actions.push(Action::AddFilesDialog);
                }
            });
        });
        ui.add_space(6.0);

        if playlist.items().is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Drop files here to queue them")
                        .font(theme::sans(12.5))
                        .color(theme::TEXT_DIM),
                );
            });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, item) in playlist.items().iter().enumerate() {
                    let current = playlist.current() == Some(index);
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ROW_HEIGHT),
                        Sense::click(),
                    );
                    let painter = ui.painter();
                    if current || response.hovered() {
                        let alpha = if current { 20 } else { 10 };
                        painter.rect_filled(rect, 8.0, Color32::from_white_alpha(alpha));
                    }
                    if current {
                        let bar = egui::Rect::from_min_size(
                            rect.left_top() + egui::vec2(0.0, 8.0),
                            Vec2::new(2.5, ROW_HEIGHT - 16.0),
                        );
                        painter.rect_filled(bar, 1.0, theme::ACCENT);
                    }
                    painter.text(
                        rect.left_center() + egui::vec2(12.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{:02}", index + 1),
                        theme::mono(11.0),
                        theme::TEXT_FAINT,
                    );
                    let title_clip = rect.with_max_x(rect.right() - 34.0);
                    painter.with_clip_rect(title_clip).text(
                        rect.left_center() + egui::vec2(40.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &item.title,
                        theme::sans(12.5),
                        if current { Color32::WHITE } else { theme::TEXT },
                    );

                    let remove = egui::Rect::from_center_size(
                        rect.right_center() - egui::vec2(16.0, 0.0),
                        Vec2::splat(22.0),
                    );
                    let remove_response = ui.interact(
                        remove,
                        ui.scope_id().with(("remove", index)),
                        Sense::click(),
                    );
                    if response.hovered() || remove_response.hovered() {
                        let color = if remove_response.hovered() {
                            Color32::WHITE
                        } else {
                            theme::TEXT_FAINT
                        };
                        Icon::Close.paint(ui.painter(), center_square(remove, 12.0), color);
                    }
                    if remove_response.clicked() {
                        actions.push(Command::RemoveIndex(index).into());
                    } else if response.clicked() {
                        actions.push(Command::PlayIndex(index).into());
                    }
                }
            });
    });
}
