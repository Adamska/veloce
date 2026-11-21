//! The speed stepper in the control bar and the popover for fine adjustment.

use egui::{Popup, PopupCloseBehavior, RectAlign, RichText, Ui};

use super::{
    ViewContext,
    action::Action,
    icons::Icon,
    theme,
    widgets::{DRAWER_OPACITY, capsule, chip, icon_button},
};
use crate::{
    audio::{MAX_SPEED, MIN_SPEED},
    player::{
        Command,
        command::{SPEED_FINE_STEP, format_speed},
    },
};

const PRESETS: [f32; 5] = [0.75, 1.0, 1.25, 1.5, 2.0];
const POPOVER_WIDTH: f32 = 292.0;
/// Lifts the popover from the speed label to just above the control capsule.
const POPOVER_GAP: f32 = 52.0;

fn popover(ui: &mut Ui, speed: f32, actions: &mut Vec<Action>) {
    ui.set_width(POPOVER_WIDTH);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Playback speed")
                .font(theme::sans(12.5))
                .color(theme::TEXT_DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format_speed(speed))
                    .font(theme::mono(15.0))
                    .color(theme::TEXT),
            );
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_button(ui, Icon::Minus, "Slower by 0.05 (Shift+[)", false).clicked() {
            actions.push(Command::AdjustSpeed(-SPEED_FINE_STEP).into());
        }
        ui.spacing_mut().slider_width = POPOVER_WIDTH - 2.0 * 30.0 - 12.0;
        let mut value = speed;
        let slider = egui::Slider::new(&mut value, MIN_SPEED..=MAX_SPEED)
            .logarithmic(true)
            .step_by(f64::from(SPEED_FINE_STEP))
            .show_value(false);
        if ui.add(slider).changed() {
            actions.push(Command::SetSpeed(value).into());
        }
        if icon_button(ui, Icon::Plus, "Faster by 0.05 (Shift+])", false).clicked() {
            actions.push(Command::AdjustSpeed(SPEED_FINE_STEP).into());
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for preset in PRESETS {
            let selected = (speed - preset).abs() < 1e-3;
            if chip(ui, &format_speed(preset), selected).clicked() {
                actions.push(Command::SetSpeed(preset).into());
            }
        }
    });
    ui.add_space(2.0);
    ui.label(
        RichText::new("Shift+[ and Shift+] step by 0.05, = resets")
            .font(theme::sans(11.0))
            .color(theme::TEXT_FAINT),
    );
}

/// A click moves to the next preset, a Shift-click by `SPEED_FINE_STEP`.
fn step_button(ui: &mut Ui, faster: bool, actions: &mut Vec<Action>) {
    let (icon, hint) = if faster {
        (Icon::Plus, "Faster (]), Shift-click for +0.05")
    } else {
        (Icon::Minus, "Slower ([), Shift-click for -0.05")
    };
    if icon_button(ui, icon, hint, false).clicked() {
        let fine = ui.input(|input| input.modifiers.shift);
        actions.push(
            match (fine, faster) {
                (true, true) => Command::AdjustSpeed(SPEED_FINE_STEP),
                (true, false) => Command::AdjustSpeed(-SPEED_FINE_STEP),
                (false, _) => Command::StepSpeed { faster },
            }
            .into(),
        );
    }
}

pub fn stepper(ui: &mut Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
    step_button(ui, true, actions);
    let speed = view.player.speed();
    let boosted = view.player.is_skipping_silence();
    let shown = if boosted {
        view.player.effective_speed()
    } else {
        speed
    };
    let color = if boosted {
        theme::WARNING
    } else if (shown - 1.0).abs() > 1e-3 {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    let label = RichText::new(format_speed(shown))
        .font(theme::mono(12.0))
        .color(color);
    let button = egui::Button::new(label)
        .frame(false)
        .min_size([46.0, 22.0].into());
    let response = ui
        .add(button)
        .on_hover_text("Adjust speed (double-click or = to reset)");
    if response.double_clicked() {
        actions.push(Command::ResetSpeed.into());
    }
    Popup::from_toggle_button_response(&response)
        .align(RectAlign::TOP)
        .gap(POPOVER_GAP)
        .frame(capsule(DRAWER_OPACITY))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| popover(ui, speed, actions));
    step_button(ui, false, actions);
}
