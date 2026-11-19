use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2};

use super::{theme, widgets::format_timecode};
use crate::{
    engine::CacheSpan,
    media::{Chapter, Micros},
};

const HEIGHT: f32 = 20.0;
const TRACK: f32 = 3.0;

pub struct ScrubberModel<'a> {
    pub position: Micros,
    pub duration: Micros,
    pub buffered: Option<CacheSpan>,
    pub chapters: &'a [Chapter],
    pub loop_points: (Option<Micros>, Option<Micros>),
    pub opacity: f32,
}

fn chapter_title(chapters: &[Chapter], time: Micros) -> Option<&str> {
    chapters
        .iter()
        .rev()
        .find(|chapter| chapter.start <= time)
        .map(|chapter| chapter.title.as_str())
}

pub fn show(ui: &mut Ui, model: &ScrubberModel<'_>) -> Option<Micros> {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), HEIGHT),
        Sense::click_and_drag(),
    );
    let duration = model.duration.max(1);
    let x_of =
        |time: Micros| rect.left() + rect.width() * (time as f32 / duration as f32).clamp(0.0, 1.0);
    let time_of = |x: f32| {
        let fraction = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        (f64::from(fraction) * duration as f64) as Micros
    };
    let active = response.hovered() || response.dragged();
    let thickness = if active { TRACK + 2.0 } else { TRACK };
    let segment = |from: f32, to: f32| {
        Rect::from_min_max(
            pos2(from, rect.center().y - thickness / 2.0),
            pos2(to.max(from), rect.center().y + thickness / 2.0),
        )
    };
    let fade = |color: Color32| theme::fade(color, model.opacity);
    let painter = ui.painter();

    painter.rect_filled(
        segment(rect.left(), rect.right()),
        2.0,
        fade(Color32::from_white_alpha(34)),
    );
    if let Some(span) = model.buffered {
        painter.rect_filled(
            segment(x_of(span.start), x_of(span.end)),
            2.0,
            fade(Color32::from_white_alpha(70)),
        );
    }
    if let (Some(start), end) = model.loop_points {
        let end_x = end.map_or(x_of(model.position), x_of);
        let band = Rect::from_min_max(
            pos2(x_of(start), rect.center().y - 6.0),
            pos2(end_x.max(x_of(start)), rect.center().y + 6.0),
        );
        painter.rect_filled(band, 2.0, fade(theme::WARNING.gamma_multiply(0.22)));
        for x in [Some(x_of(start)), end.map(x_of)].into_iter().flatten() {
            painter.vline(x, band.y_range(), Stroke::new(1.5, fade(theme::WARNING)));
        }
    }
    painter.rect_filled(
        segment(rect.left(), x_of(model.position)),
        2.0,
        fade(theme::ACCENT),
    );
    for chapter in model.chapters.iter().filter(|chapter| chapter.start > 0) {
        let x = x_of(chapter.start);
        painter.vline(
            x,
            rect.center().y - 5.0..=rect.center().y + 5.0,
            Stroke::new(1.0, fade(Color32::from_white_alpha(150))),
        );
    }
    if active {
        painter.circle_filled(
            pos2(x_of(model.position), rect.center().y),
            6.0,
            fade(Color32::WHITE),
        );
    }

    if let Some(pointer) = response.hover_pos().filter(|_| active) {
        let time = time_of(pointer.x);
        let label = match chapter_title(model.chapters, time) {
            Some(title) => format!("{}  {title}", format_timecode(time)),
            None => format_timecode(time),
        };
        let galley = painter.layout_no_wrap(label, theme::mono(11.0), Color32::WHITE);
        let size = galley.size() + Vec2::new(16.0, 10.0);
        let center_x = pointer
            .x
            .clamp(rect.left() + size.x / 2.0, rect.right() - size.x / 2.0);
        let bubble = Rect::from_center_size(pos2(center_x, rect.top() - size.y / 2.0 - 6.0), size);
        painter.rect_filled(bubble, 7.0, theme::capsule_fill(1.15));
        painter.rect_stroke(bubble, 7.0, theme::specular(1.0), egui::StrokeKind::Inside);
        painter.galley(
            Align2::CENTER_CENTER
                .anchor_size(bubble.center(), galley.size())
                .min,
            galley,
            Color32::WHITE,
        );
        painter.vline(
            pointer.x,
            rect.center().y - 6.0..=rect.center().y + 6.0,
            Stroke::new(1.0, Color32::from_white_alpha(200)),
        );
    }

    let pointer: Option<Pos2> = response.interact_pointer_pos();
    if (response.dragged() || response.clicked())
        && let Some(pointer) = pointer
    {
        return Some(time_of(pointer.x));
    }
    None
}
