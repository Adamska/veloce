use egui::{Align2, Area, Id, Key, Order, RichText};

use super::{action::Action, theme, widgets::capsule};
use crate::player::Command;

#[derive(Default)]
pub struct NetworkDialog {
    url: String,
    open: bool,
    focus_pending: bool,
}

impl NetworkDialog {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.focus_pending = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn show(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.open {
            return;
        }
        let mut submit = false;
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            self.open = false;
            return;
        }
        Area::new(Id::unique("network stream"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                capsule(1.15).show(ui, |ui| {
                    ui.set_width(460.0);
                    ui.label(
                        RichText::new("Open Network Stream")
                            .font(theme::sans(15.0))
                            .color(theme::TEXT),
                    );
                    ui.label(
                        RichText::new("HTTP, HTTPS, HLS (.m3u8) and RTSP addresses are supported")
                            .font(theme::sans(11.5))
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(6.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.url)
                            .hint_text("https://example.com/stream.m3u8")
                            .font(theme::mono(12.0))
                            .desired_width(f32::INFINITY),
                    );
                    if self.focus_pending {
                        field.request_focus();
                        self.focus_pending = false;
                    }
                    submit |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        submit |= ui.button("Open").clicked();
                        if ui.button("Cancel").clicked() {
                            self.open = false;
                        }
                    });
                });
            });
        let url = self.url.trim();
        if submit && !url.is_empty() {
            actions.push(Command::Open(vec![url.to_owned()]).into());
            self.open = false;
        }
    }
}
