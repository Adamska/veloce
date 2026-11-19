use std::path::Path;

use egui::{
    Align2, Color32, CursorIcon, Rect, Sense, Stroke, Ui, pos2,
    text::{LayoutJob, TextWrapping},
    vec2,
};

use super::{action::Action, icons::Icon, theme};
use crate::player::{Command, RecentEntry};

pub const VISIBLE_ROWS: usize = 6;
const ROW_HEIGHT: f32 = 48.0;
const AUDIO_EXTENSIONS: [&str; 10] = [
    "flac", "mp3", "m4a", "aac", "ogg", "opus", "wav", "wma", "mka", "aiff",
];

pub fn ago(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then);
    let (minutes, hours, days) = (seconds / 60, seconds / 3_600, seconds / 86_400);
    match () {
        () if minutes < 1 => "Just now".to_owned(),
        () if hours < 1 => format!("{minutes} min ago"),
        () if days < 1 => format!("{hours} h ago"),
        () if days == 1 => "Yesterday".to_owned(),
        () if days < 7 => format!("{days} days ago"),
        () if days < 14 => "Last week".to_owned(),
        () if days < 60 => format!("{} weeks ago", days / 7),
        () if days < 730 => format!("{} months ago", days / 30),
        () => format!("{} years ago", days / 365),
    }
}

pub fn folder_label(path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home
        .as_deref()
        .and_then(|home| parent.strip_prefix(home).ok())
    {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => parent.display().to_string(),
    }
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| AUDIO_EXTENSIONS.contains(&extension.to_lowercase().as_str()))
}

fn line(text: String, size: f32, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text, theme::sans(size), color);
    job.wrap = TextWrapping::truncate_at_width(width.max(0.0));
    job
}

fn row(ui: &mut Ui, entry: &RecentEntry, now: u64, actions: &mut Vec<Action>) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let forget_rect =
        Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(24.0, 24.0));
    let forget = ui.interact(
        forget_rect,
        ui.scope_id().with(("forget", &entry.path)),
        Sense::click(),
    );
    let hovered = response.hovered() || forget.hovered();
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, 10.0, Color32::from_white_alpha(10));
    }

    let badge = Rect::from_center_size(pos2(rect.left() + 28.0, rect.center().y), vec2(32.0, 32.0));
    painter.rect(
        badge,
        9.0,
        Color32::from_white_alpha(10),
        Stroke::new(1.0, Color32::from_white_alpha(18)),
        egui::StrokeKind::Inside,
    );
    let glyph = if is_audio(&entry.path) {
        Icon::Volume
    } else {
        Icon::Play
    };
    glyph.paint(painter, badge.shrink(9.0), theme::TEXT_DIM);

    let text_left = badge.right() + 14.0;
    let trailing = 96.0;
    let width = rect.right() - trailing - text_left;
    let name = entry.path.file_name().map_or_else(
        || entry.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let name = painter.layout_job(line(name, 13.5, theme::TEXT, width));
    let folder = painter.layout_job(line(
        folder_label(&entry.path),
        11.0,
        theme::TEXT_FAINT,
        width,
    ));
    let top = rect.center().y - (name.size().y + folder.size().y + 2.0) / 2.0;
    painter.galley(pos2(text_left, top), name.clone(), theme::TEXT);
    painter.galley(
        pos2(text_left, top + name.size().y + 2.0),
        folder,
        theme::TEXT_FAINT,
    );

    if hovered {
        let color = if forget.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        Icon::Close.paint(painter, forget_rect.shrink(6.0), color);
    } else {
        painter.text(
            pos2(rect.right() - 14.0, rect.center().y),
            Align2::RIGHT_CENTER,
            ago(now, entry.opened),
            theme::sans(11.0),
            theme::TEXT_FAINT,
        );
    }

    if forget.on_hover_text("Remove from recent files").clicked() {
        actions.push(Command::ForgetRecent(entry.path.clone()).into());
    } else if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        actions.push(Command::Open(vec![entry.path.display().to_string()]).into());
    }
}

pub fn show(ui: &mut Ui, entries: &[RecentEntry], now: u64, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Recent")
                .font(theme::sans(12.0))
                .color(theme::TEXT_DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = egui::Button::new(
                egui::RichText::new("Clear")
                    .font(theme::sans(11.5))
                    .color(theme::TEXT_FAINT),
            )
            .frame(false);
            if ui
                .add(clear)
                .on_hover_text("Forget all recent files")
                .clicked()
            {
                actions.push(Command::ClearRecent.into());
            }
        });
    });
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(Color32::from_white_alpha(5))
        .stroke(theme::specular(1.0))
        .corner_radius(theme::CAPSULE_RADIUS)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for entry in entries.iter().take(VISIBLE_ROWS) {
                row(ui, entry, now, actions);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn elapsed_time_reads_naturally() {
        let now = 10_000_000;
        let cases = [
            (now - 20, "Just now"),
            (now - 5 * 60, "5 min ago"),
            (now - 3 * 3_600, "3 h ago"),
            (now - 30 * 3_600, "Yesterday"),
            (now - 4 * 86_400, "4 days ago"),
            (now - 9 * 86_400, "Last week"),
            (now - 40 * 86_400, "5 weeks ago"),
            (now - 100 * 86_400, "3 months ago"),
            (now + 50, "Just now"),
        ];
        for (then, expected) in cases {
            assert_eq!(ago(now, then), expected);
        }
    }
}
