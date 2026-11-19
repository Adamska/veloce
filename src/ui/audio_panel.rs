use std::sync::atomic::Ordering;

use egui::{RichText, Ui};

use super::{
    ViewContext,
    action::Action,
    theme,
    widgets::{chip, section, slider_row},
};
use crate::{
    audio::equalizer::{BAND_COUNT, BAND_FREQUENCIES, GAIN_RANGE_DB, PREAMP_RANGE_DB, PRESETS},
    player::{
        Command,
        command::{AUDIO_DELAY_STEP, format_delay},
    },
};

fn band_label(frequency: f32) -> String {
    if frequency >= 1_000.0 {
        format!("{:.0}k", frequency / 1_000.0)
    } else {
        format!("{frequency:.0}")
    }
}

fn equalizer(ui: &mut Ui, view: &ViewContext<'_>) {
    let settings = &view.player.controls().equalizer;
    section(ui, "10-band equalizer");
    let mut enabled = settings.is_enabled();
    if ui.checkbox(&mut enabled, "Enable equalizer").changed() {
        settings.set_enabled(enabled);
    }
    let gains = settings.gains_db();
    ui.horizontal_wrapped(|ui| {
        for (name, preset) in &PRESETS {
            if chip(ui, name, gains == *preset).clicked() {
                settings.apply_preset(preset);
                settings.set_enabled(true);
            }
        }
    });
    let mut preamp = settings.preamp_db();
    let range = -PREAMP_RANGE_DB..=PREAMP_RANGE_DB;
    if slider_row(ui, "Preamp", &mut preamp, range, |v| format!("{v:+.1} dB")) {
        settings.set_preamp_db(preamp);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().slider_width = 110.0;
        for band in 0..BAND_COUNT {
            ui.vertical(|ui| {
                let mut gain = gains[band];
                let slider = egui::Slider::new(&mut gain, -GAIN_RANGE_DB..=GAIN_RANGE_DB)
                    .vertical()
                    .trailing_fill(false)
                    .show_value(false);
                if ui
                    .add(slider)
                    .on_hover_text(format!("{gain:+.1} dB"))
                    .changed()
                {
                    settings.set_gain_db(band, gain);
                    settings.set_enabled(true);
                }
                ui.label(
                    RichText::new(band_label(BAND_FREQUENCIES[band]))
                        .font(theme::mono(9.5))
                        .color(theme::TEXT_DIM),
                );
            });
        }
    });
}

fn silence_skipper(ui: &mut Ui, view: &ViewContext<'_>) {
    let settings = &view.player.controls().silence;
    section(ui, "Smart silence skipper");
    let mut enabled = settings.enabled.load(Ordering::Relaxed);
    if ui
        .checkbox(&mut enabled, "Accelerate silent pauses")
        .changed()
    {
        settings.enabled.store(enabled, Ordering::Relaxed);
    }
    let mut threshold = settings.threshold_db.load();
    if slider_row(ui, "Threshold", &mut threshold, -70.0..=-20.0, |v| {
        format!("{v:.0} dB")
    }) {
        settings.threshold_db.store(threshold);
    }
    let mut boost = settings.boost_speed.load();
    if slider_row(ui, "Skip speed", &mut boost, 1.5..=4.0, |v| {
        format!("{v:.2}x")
    }) {
        settings.boost_speed.store(boost);
    }
}

pub fn show(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
    let player = view.player;
    section(ui, "Audio track");
    let selected = player.audio_track();
    let tracks = player.info().map_or(&[][..], |info| &info.audio_tracks);
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Off", selected.is_none()).clicked() {
            actions.push(Command::SelectAudioTrack(None).into());
        }
        for (index, track) in tracks.iter().enumerate() {
            if chip(ui, &track.label(index), selected == Some(index)).clicked() {
                actions.push(Command::SelectAudioTrack(Some(index)).into());
            }
        }
    });

    section(ui, "Level and sync");
    let mut volume = player.volume() * 100.0;
    if slider_row(ui, "Volume", &mut volume, 0.0..=200.0, |v| {
        format!("{v:.0} %")
    }) {
        actions.push(Command::SetVolume(volume / 100.0).into());
    }
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Audio delay")
                .font(theme::sans(12.0))
                .color(theme::TEXT_DIM),
        );
        if ui.button("-25 ms").on_hover_text("J").clicked() {
            actions.push(Command::AdjustAudioDelay(-AUDIO_DELAY_STEP).into());
        }
        ui.label(
            RichText::new(format_delay(player.audio_delay()))
                .font(theme::mono(12.0))
                .color(theme::TEXT),
        );
        if ui.button("+25 ms").on_hover_text("K").clicked() {
            actions.push(Command::AdjustAudioDelay(AUDIO_DELAY_STEP).into());
        }
    });

    equalizer(ui, view);
    silence_skipper(ui, view);
}
