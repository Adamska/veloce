use egui::{RichText, ScrollArea, Ui};

use super::{
    ViewContext,
    action::Action,
    audio_panel,
    icons::Icon,
    subtitle_panel, theme,
    widgets::{chip, drawer, icon_button, section, slider_row},
};
use crate::{
    gpu::{AspectMode, CropPreset, Geometry, VideoAdjustments},
    player::Command,
};

const WIDTH: f32 = 380.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Video,
    Audio,
    Subtitles,
}

fn video_tab(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
    let mut adjust = view.player.adjustments;
    let mut geometry = view.player.geometry;

    section(ui, "Picture");
    slider_row(ui, "Brightness", &mut adjust.brightness, -0.5..=0.5, |v| {
        format!("{v:+.2}")
    });
    slider_row(ui, "Contrast", &mut adjust.contrast, 0.0..=2.0, |v| {
        format!("{v:.2}")
    });
    slider_row(ui, "Saturation", &mut adjust.saturation, 0.0..=2.0, |v| {
        format!("{v:.2}")
    });
    slider_row(ui, "Gamma", &mut adjust.gamma, 0.4..=2.5, |v| {
        format!("{v:.2}")
    });
    slider_row(ui, "Hue", &mut adjust.hue_degrees, -180.0..=180.0, |v| {
        format!("{v:+.0} deg")
    });
    slider_row(ui, "Sharpen", &mut adjust.sharpen, 0.0..=2.0, |v| {
        format!("{v:.2}")
    });
    ui.checkbox(&mut adjust.deinterlace, "Deinterlace (linear blend)");
    ui.checkbox(&mut adjust.frame_blending, "Blend frames at 2.0x and above");
    if ui.button("Reset picture").clicked() {
        adjust = VideoAdjustments::default();
    }

    section(ui, "Aspect ratio");
    ui.horizontal_wrapped(|ui| {
        for mode in AspectMode::ALL {
            if chip(ui, mode.label(), geometry.aspect == mode).clicked() {
                geometry.aspect = mode;
            }
        }
    });
    section(ui, "Crop");
    ui.horizontal_wrapped(|ui| {
        for preset in CropPreset::ALL {
            if chip(ui, preset.label(), geometry.crop == preset).clicked() {
                geometry.crop = preset;
            }
        }
    });
    section(ui, "Transform");
    slider_row(ui, "Zoom", &mut geometry.zoom, 1.0..=4.0, |v| {
        format!("{v:.2}x")
    });
    slider_row(ui, "Pan X", &mut geometry.pan[0], -1.0..=1.0, |v| {
        format!("{v:+.2}")
    });
    slider_row(ui, "Pan Y", &mut geometry.pan[1], -1.0..=1.0, |v| {
        format!("{v:+.2}")
    });
    ui.horizontal(|ui| {
        if ui.button("Rotate 90").clicked() {
            geometry.rotate_clockwise();
        }
        ui.checkbox(&mut geometry.mirror, "Mirror");
        if ui.button("Reset geometry").clicked() {
            geometry = Geometry::default();
        }
    });

    if adjust != view.player.adjustments {
        actions.push(Command::SetAdjustments(adjust).into());
    }
    if geometry != view.player.geometry {
        actions.push(Command::SetGeometry(geometry).into());
    }
}

pub fn show(
    ctx: &egui::Context,
    view: &ViewContext<'_>,
    openness: f32,
    tab: &mut Tab,
    actions: &mut Vec<Action>,
) {
    drawer(ctx, "filters drawer", openness, WIDTH, |ui| {
        ui.horizontal(|ui| {
            for (candidate, label) in [
                (Tab::Video, "Video"),
                (Tab::Audio, "Audio"),
                (Tab::Subtitles, "Subtitles"),
            ] {
                let text = RichText::new(label).font(theme::sans(14.0));
                if ui.selectable_label(*tab == candidate, text).clicked() {
                    *tab = candidate;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, "Close (E)", false).clicked() {
                    actions.push(Action::ToggleFilters);
                }
            });
        });
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match tab {
                Tab::Video => video_tab(ui, view, actions),
                Tab::Audio => audio_panel::show(ui, view, actions),
                Tab::Subtitles => subtitle_panel::show(ui, view, actions),
            });
    });
}
