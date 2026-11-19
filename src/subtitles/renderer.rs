use egui::{Align, Color32, FontFamily, FontId, Pos2, Rect, Vec2, text::LayoutJob};

const REFERENCE_WIDTH: f32 = 1280.0;
const WRAP_FRACTION: f32 = 0.82;
const OUTLINE: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubtitleStyle {
    pub font_size: f32,
    pub vertical_position: f32,
    pub backdrop_opacity: f32,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_size: 30.0,
            vertical_position: 0.07,
            backdrop_opacity: 0.55,
        }
    }
}

impl SubtitleStyle {
    pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 14.0..=72.0;
    pub const POSITION_RANGE: std::ops::RangeInclusive<f32> = 0.0..=0.9;
}

pub fn paint(painter: &egui::Painter, area: Rect, lines: &[String], style: &SubtitleStyle) {
    if lines.is_empty() || area.height() < 1.0 {
        return;
    }
    let font = FontId::new(
        (style.font_size * area.width() / REFERENCE_WIDTH).clamp(10.0, 160.0),
        FontFamily::Proportional,
    );
    let layout = |color: Color32| {
        let mut job = LayoutJob::simple(
            lines.join("\n"),
            font.clone(),
            color,
            area.width() * WRAP_FRACTION,
        );
        job.halign = Align::Center;
        painter.layout_job(job)
    };
    let text = layout(Color32::WHITE);
    let size = text.size();
    let baseline = area.bottom() - area.height() * style.vertical_position.clamp(0.0, 0.95);
    let anchor = Pos2::new(area.center().x, (baseline - size.y).max(area.top()));

    let padding = Vec2::new(font.size * 0.45, font.size * 0.22);
    let backdrop = Rect::from_center_size(
        Pos2::new(anchor.x, anchor.y + size.y / 2.0),
        size + padding * 2.0,
    );
    let backdrop_alpha = (style.backdrop_opacity.clamp(0.0, 1.0) * 255.0) as u8;
    if backdrop_alpha > 0 {
        painter.rect_filled(
            backdrop,
            font.size * 0.18,
            Color32::from_black_alpha(backdrop_alpha),
        );
    }
    if backdrop_alpha < 200 {
        let outline = layout(Color32::from_black_alpha(220));
        let spread = (font.size / 22.0).max(1.0);
        for (dx, dy) in OUTLINE {
            painter.galley(
                anchor + Vec2::new(dx, dy) * spread,
                outline.clone(),
                Color32::BLACK,
            );
        }
    }
    painter.galley(anchor, text, Color32::WHITE);
}
