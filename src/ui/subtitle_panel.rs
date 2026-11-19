use egui::{RichText, Ui};

use super::{
    ViewContext,
    action::Action,
    theme,
    widgets::{chip, section, slider_row},
};
use crate::{
    player::{
        Command,
        command::{SUBTITLE_DELAY_STEP, format_delay},
    },
    subtitles::SubtitleStyle,
};

pub fn show(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
    let subtitles = &view.player.subtitles;

    section(ui, "Subtitle track");
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Off", subtitles.selection.is_none()).clicked() {
            actions.push(Command::SelectSubtitles(None).into());
        }
        for (selection, label) in subtitles.choices() {
            if chip(ui, &label, subtitles.selection == Some(selection)).clicked() {
                actions.push(Command::SelectSubtitles(Some(selection)).into());
            }
        }
    });
    ui.add_space(4.0);
    if ui.button("Load subtitle file").clicked() {
        actions.push(Action::OpenSubtitleDialog);
    }
    ui.label(
        RichText::new("SRT, VTT and ASS files can also be dropped onto the window")
            .font(theme::sans(11.0))
            .color(theme::TEXT_FAINT),
    );

    section(ui, "Sync");
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Subtitle delay")
                .font(theme::sans(12.0))
                .color(theme::TEXT_DIM),
        );
        if ui.button("-50 ms").on_hover_text("G").clicked() {
            actions.push(Command::AdjustSubtitleDelay(-SUBTITLE_DELAY_STEP).into());
        }
        ui.label(
            RichText::new(format_delay(subtitles.delay))
                .font(theme::mono(12.0))
                .color(theme::TEXT),
        );
        if ui.button("+50 ms").on_hover_text("H").clicked() {
            actions.push(Command::AdjustSubtitleDelay(SUBTITLE_DELAY_STEP).into());
        }
    });

    section(ui, "Appearance");
    let mut style = subtitles.style;
    slider_row(
        ui,
        "Font size",
        &mut style.font_size,
        SubtitleStyle::FONT_SIZE_RANGE,
        |v| format!("{v:.0} pt"),
    );
    slider_row(
        ui,
        "Position",
        &mut style.vertical_position,
        SubtitleStyle::POSITION_RANGE,
        |v| format!("{:.0} %", v * 100.0),
    );
    slider_row(
        ui,
        "Backdrop",
        &mut style.backdrop_opacity,
        0.0..=1.0,
        |v| format!("{:.0} %", v * 100.0),
    );
    if ui.button("Reset appearance").clicked() {
        style = SubtitleStyle::default();
    }
    if style != subtitles.style {
        actions.push(Command::SetSubtitleStyle(style).into());
    }
}
