use egui::{Color32, Frame, Margin, Response, RichText, Sense, Ui, Vec2};

use super::{
    icons::{Icon, center_square},
    theme,
};
use crate::media::Micros;

pub const ICON_BUTTON: f32 = 30.0;
pub const HUD_RESERVE: f32 = 122.0;
const LABEL_WIDTH: f32 = 88.0;
const DRAWER_OPACITY: f32 = 1.14;

pub fn format_timecode(time: Micros) -> String {
    let millis = time.max(0) / 1_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1_000 % 60,
        millis % 1_000
    )
}

pub fn format_offset(time: Micros) -> String {
    format!("{:+.1} s", time as f64 / 1e6)
}

pub fn capsule(opacity: f32) -> Frame {
    Frame::new()
        .fill(theme::capsule_fill(opacity))
        .stroke(theme::specular(opacity))
        .corner_radius(theme::CAPSULE_RADIUS)
        .inner_margin(Margin::symmetric(14, 10))
}

pub fn icon_button(ui: &mut Ui, icon: Icon, hint: &str, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ICON_BUTTON), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() || active {
            let strength = if response.is_pointer_button_down_on() {
                30
            } else {
                16
            };
            painter.rect_filled(rect, 8.0, Color32::from_white_alpha(strength));
        }
        let color = if active {
            theme::ACCENT
        } else if response.hovered() {
            Color32::WHITE
        } else {
            theme::TEXT
        };
        icon.paint(painter, center_square(rect, 16.0), color);
    }
    response.on_hover_text(hint)
}

pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let text = RichText::new(label)
        .font(theme::sans(12.0))
        .color(if selected {
            Color32::WHITE
        } else {
            theme::TEXT_DIM
        });
    ui.selectable_label(selected, text)
}

pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .font(theme::sans(10.5))
            .color(theme::TEXT_FAINT),
    );
    ui.add_space(2.0);
}

pub fn value_row(ui: &mut Ui, label: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .font(theme::mono(11.0))
                .color(theme::TEXT_DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .font(theme::mono(11.0))
                    .color(theme::TEXT),
            );
        });
    });
}

pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    display: impl Fn(f32) -> String,
) -> bool {
    ui.horizontal(|ui| {
        let (label_rect, _) = ui.allocate_exact_size(Vec2::new(LABEL_WIDTH, 18.0), Sense::hover());
        ui.painter().text(
            label_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            theme::sans(12.0),
            theme::TEXT_DIM,
        );
        let changed = ui
            .add(egui::Slider::new(value, range).show_value(false))
            .changed();
        ui.label(
            RichText::new(display(*value))
                .font(theme::mono(11.0))
                .color(theme::TEXT),
        );
        changed
    })
    .inner
}

pub fn drawer(
    ctx: &egui::Context,
    id: &'static str,
    openness: f32,
    width: f32,
    add_contents: impl FnOnce(&mut Ui),
) {
    if openness <= 0.001 {
        return;
    }
    let screen = ctx.content_rect();
    let margin = 16.0;
    let left = screen.right() - (width + margin) * openness;
    egui::Area::new(egui::Id::unique(id))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(left, screen.top() + margin))
        .show(ctx, |ui| {
            ui.multiply_opacity(openness);
            capsule(DRAWER_OPACITY).show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.set_height((screen.height() - margin - HUD_RESERVE - 20.0).max(120.0));
                add_contents(ui);
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecodes_use_fixed_width_millisecond_precision() {
        assert_eq!(format_timecode(0), "00:00:00.000");
        assert_eq!(format_timecode(862_480_000), "00:14:22.480");
        assert_eq!(format_timecode(6_130_000_000), "01:42:10.000");
        assert_eq!(format_timecode(-5), "00:00:00.000");
        assert_eq!(format_offset(-1_250_000), "-1.2 s");
    }
}
