use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};

pub const CANVAS: Color32 = Color32::from_rgb(0x06, 0x07, 0x09);
pub const TEXT: Color32 = Color32::from_rgb(0xEC, 0xEE, 0xF2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8B, 0x91, 0x9C);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5A, 0x60, 0x6B);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x3D);
pub const WARNING: Color32 = Color32::from_rgb(0xF2, 0xC1, 0x4E);
pub const CAPSULE_RADIUS: f32 = 14.0;

pub const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 6.0 / 255.0,
    g: 7.0 / 255.0,
    b: 9.0 / 255.0,
    a: 1.0,
};

pub fn capsule_fill(opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(14, 16, 21, (0.82 * opacity * 255.0) as u8)
}

pub fn specular(opacity: f32) -> Stroke {
    Stroke::new(
        1.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, (0.10 * opacity * 255.0) as u8),
    )
}

pub fn fade(color: Color32, opacity: f32) -> Color32 {
    color.gamma_multiply(opacity.clamp(0.0, 1.0))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/InterVariable.ttf"
        ))),
    );
    fonts.font_data.insert(
        "JetBrains Mono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "Inter".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.insert(0, "JetBrains Mono".to_owned());
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, sans(20.0)),
            (TextStyle::Body, sans(13.0)),
            (TextStyle::Button, sans(13.0)),
            (TextStyle::Small, sans(11.0)),
            (TextStyle::Monospace, mono(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.slider_width = 150.0;
        style.spacing.interact_size.y = 22.0;

        let visuals = &mut style.visuals;
        *visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT);
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = capsule_fill(1.0);
        visuals.window_stroke = specular(1.0);
        visuals.window_corner_radius = CornerRadius::same(CAPSULE_RADIUS as u8);
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.popup_shadow = egui::Shadow::NONE;
        visuals.extreme_bg_color = Color32::from_rgb(0x0B, 0x0D, 0x11);
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.slider_trailing_fill = true;

        let radius = CornerRadius::same(7);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_stroke = specular(1.0);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        widgets.inactive.bg_fill = Color32::from_white_alpha(30);
        widgets.inactive.weak_bg_fill = Color32::from_white_alpha(12);
        widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(26));
        widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.hovered.bg_fill = Color32::from_white_alpha(44);
        widgets.hovered.weak_bg_fill = Color32::from_white_alpha(18);
        widgets.hovered.bg_stroke = specular(1.6);
        widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        widgets.active.bg_fill = Color32::from_white_alpha(56);
        widgets.active.weak_bg_fill = Color32::from_white_alpha(28);
        widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        for state in [
            &mut widgets.noninteractive,
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
    });
}
