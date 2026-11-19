use egui::{Align2, Color32, Id, LayerId, Order};

use super::{ViewContext, theme};

pub fn show(ctx: &egui::Context, view: &ViewContext<'_>, opacity: f32) {
    let player = view.player;
    let Some(info) = player.info().filter(|_| opacity > 0.001) else {
        return;
    };
    let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("title")));
    let origin = ctx.content_rect().left_top() + egui::vec2(24.0, 40.0);
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
