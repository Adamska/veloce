use egui::{Align2, Area, Id, Order, RichText};

use super::{theme, widgets::capsule};
use crate::player::Notice;

const VISIBLE_SECONDS: f64 = 1.8;
const ERROR_SECONDS: f64 = 6.0;
const FADE_SECONDS: f64 = 0.25;
const MAX_VISIBLE: usize = 3;

#[derive(Default)]
pub struct Toasts {
    items: Vec<(Notice, f64)>,
}

impl Toasts {
    pub fn push(&mut self, notice: Notice, now: f64) {
        let lifetime = if notice.is_error {
            ERROR_SECONDS
        } else {
            VISIBLE_SECONDS
        };
        let family = |text: &str| text.split([' ', ':']).next().unwrap_or_default().to_owned();
        self.items.retain(|(existing, _)| {
            existing.is_error || family(&existing.text) != family(&notice.text)
        });
        self.items.push((notice, now + lifetime));
        let overflow = self.items.len().saturating_sub(MAX_VISIBLE);
        self.items.drain(..overflow);
    }

    pub fn show(&mut self, ctx: &egui::Context, now: f64) {
        self.items.retain(|(_, expiry)| *expiry > now);
        if self.items.is_empty() {
            return;
        }
        Area::new(Id::unique("toasts"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_TOP, [0.0, 22.0])
            .interactable(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    for (notice, expiry) in &self.items {
                        let opacity = ((expiry - now) / FADE_SECONDS).clamp(0.0, 1.0) as f32;
                        ui.scope(|ui| {
                            ui.multiply_opacity(opacity);
                            capsule(1.0).show(ui, |ui| {
                                let color = if notice.is_error {
                                    theme::WARNING
                                } else {
                                    theme::TEXT
                                };
                                ui.label(
                                    RichText::new(&notice.text)
                                        .font(theme::mono(12.5))
                                        .color(color),
                                );
                            });
                        });
                        ui.add_space(4.0);
                    }
                });
            });
        ctx.request_repaint();
    }
}
