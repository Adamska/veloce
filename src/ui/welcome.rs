use egui::{Align2, Area, Id, Order, RichText, Ui};

use super::{action::Action, theme};
use crate::player::Command;

const SHORTCUTS: [(&str, &str); 8] = [
    ("Space", "Play / Pause"),
    ("Left / Right", "Seek 5 s"),
    ("Shift + Left / Right", "Seek 30 s"),
    (", / .", "Frame step"),
    ("[ / ]", "Speed"),
    ("L", "A-B loop"),
    ("F", "Fullscreen"),
    ("Cmd/Ctrl + I", "Inspector"),
];

fn bracket_button(ui: &mut Ui, label: &str, primary: bool) -> bool {
    let color = if primary { theme::ACCENT } else { theme::TEXT };
    let text = RichText::new(format!("[ {label} ]"))
        .font(theme::mono(13.0))
        .color(color);
    ui.add(egui::Button::new(text).frame(false)).clicked()
}

pub fn show(ctx: &egui::Context, actions: &mut Vec<Action>) {
    Area::new(Id::unique("welcome"))
        .order(Order::Middle)
        .anchor(Align2::CENTER_CENTER, [0.0, -10.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Veloce")
                        .font(theme::sans(44.0))
                        .color(theme::TEXT),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Drop media anywhere, or pick a source")
                        .font(theme::sans(13.0))
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(26.0);
                if bracket_button(ui, "Open File", false) {
                    actions.push(Action::OpenFileDialog);
                }
                ui.add_space(4.0);
                if bracket_button(ui, "Open Network Stream", false) {
                    actions.push(Action::ShowNetworkDialog);
                }
                ui.add_space(4.0);
                if bracket_button(ui, "Launch 4K Scrubbing Demo", true) {
                    actions.push(Command::OpenDemo.into());
                }
                ui.add_space(30.0);
                egui::Grid::new("welcome shortcuts")
                    .num_columns(2)
                    .spacing([18.0, 5.0])
                    .show(ui, |ui| {
                        for (keys, meaning) in SHORTCUTS {
                            ui.label(
                                RichText::new(keys)
                                    .font(theme::mono(11.0))
                                    .color(theme::TEXT_DIM),
                            );
                            ui.label(
                                RichText::new(meaning)
                                    .font(theme::sans(11.5))
                                    .color(theme::TEXT_FAINT),
                            );
                            ui.end_row();
                        }
                    });
            });
        });
}
