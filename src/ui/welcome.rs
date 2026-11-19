use egui::{
    Align2, Area, Color32, CursorIcon, Id, LayerId, Mesh, Order, Pos2, Rect, Response, RichText,
    Stroke, TextureHandle, Ui, epaint::Vertex, pos2, vec2,
};

use super::{action::Action, recent_list, theme};
use crate::player::{RecentEntry, recent::now_seconds};

const MAX_WIDTH: f32 = 480.0;
const LOGO_SIZE: f32 = 104.0;
const BACKDROP_TOP: Color32 = Color32::from_rgb(0x17, 0x1A, 0x21);
const BACKDROP_BOTTOM: Color32 = Color32::from_rgb(0x0D, 0x0F, 0x13);
const GLOW_RINGS: usize = 6;
const GLOW_SEGMENTS: usize = 72;

fn vertex(pos: Pos2, color: Color32) -> Vertex {
    Vertex {
        pos,
        uv: egui::epaint::WHITE_UV,
        color,
    }
}

fn vertical_gradient(mesh: &mut Mesh, rect: Rect, top: Color32, bottom: Color32) {
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend([
        vertex(rect.left_top(), top),
        vertex(rect.right_top(), top),
        vertex(rect.right_bottom(), bottom),
        vertex(rect.left_bottom(), bottom),
    ]);
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// A soft radial light whose intensity eases out towards `radius`.
fn glow(mesh: &mut Mesh, center: Pos2, radius: Pos2, color: Color32) {
    let ring_color = |ring: usize| {
        let t = ring as f32 / GLOW_RINGS as f32;
        color.gamma_multiply((1.0 - t * t * (3.0 - 2.0 * t)).max(0.0))
    };
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(vertex(center, color));
    for ring in 1..=GLOW_RINGS {
        let scale = ring as f32 / GLOW_RINGS as f32;
        for segment in 0..GLOW_SEGMENTS {
            let angle = segment as f32 / GLOW_SEGMENTS as f32 * std::f32::consts::TAU;
            let offset = vec2(angle.cos() * radius.x, angle.sin() * radius.y) * scale;
            mesh.vertices
                .push(vertex(center + offset, ring_color(ring)));
        }
    }
    let at = |ring: usize, segment: usize| {
        base + 1 + ((ring - 1) * GLOW_SEGMENTS + segment % GLOW_SEGMENTS) as u32
    };
    for segment in 0..GLOW_SEGMENTS {
        mesh.indices
            .extend([base, at(1, segment), at(1, segment + 1)]);
        for ring in 2..=GLOW_RINGS {
            let (a, b) = (at(ring - 1, segment), at(ring - 1, segment + 1));
            let (c, d) = (at(ring, segment), at(ring, segment + 1));
            mesh.indices.extend([a, c, d, a, d, b]);
        }
    }
}

fn paint_backdrop(ctx: &egui::Context, focus: Pos2) {
    let screen = ctx.content_rect();
    let mut mesh = Mesh::default();
    vertical_gradient(&mut mesh, screen, BACKDROP_TOP, BACKDROP_BOTTOM);
    let reach = screen.width().max(screen.height()) * 0.55;
    glow(
        &mut mesh,
        focus,
        pos2(reach, reach * 0.8),
        Color32::from_rgba_unmultiplied(34, 190, 160, 30),
    );
    glow(
        &mut mesh,
        pos2(screen.right() - screen.width() * 0.2, screen.bottom()),
        pos2(reach * 0.9, reach * 0.6),
        Color32::from_rgba_unmultiplied(255, 176, 64, 10),
    );
    ctx.layer_painter(LayerId::new(
        Order::Background,
        Id::unique("welcome backdrop"),
    ))
    .add(mesh);
}

fn pill(ui: &mut Ui, label: &str, primary: bool) -> Response {
    let (text, fill, stroke) = if primary {
        (
            Color32::from_rgb(0x1C, 0x10, 0x06),
            theme::ACCENT,
            Stroke::NONE,
        )
    } else {
        (
            theme::TEXT,
            Color32::from_white_alpha(14),
            theme::specular(1.0),
        )
    };
    let button = egui::Button::new(RichText::new(label).font(theme::sans(13.5)).color(text))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(10.0)
        .min_size(vec2(160.0, 38.0));
    ui.add(button).on_hover_cursor(CursorIcon::PointingHand)
}

fn shortcut(key: &str) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    format!("{modifier}+{key}")
}

pub fn show(
    ctx: &egui::Context,
    logo: Option<&TextureHandle>,
    recent: &[RecentEntry],
    actions: &mut Vec<Action>,
) {
    let screen = ctx.content_rect();
    let width = MAX_WIDTH.min(screen.width() - 48.0);
    let mut logo_center = screen.center() - vec2(0.0, 80.0);
    Area::new(Id::unique("welcome"))
        .order(Order::Middle)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.vertical_centered(|ui| {
                if let Some(logo) = logo {
                    let image =
                        egui::Image::new(logo).fit_to_exact_size(vec2(LOGO_SIZE, LOGO_SIZE));
                    logo_center = ui.add(image).rect.center();
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Veloce")
                        .font(theme::sans(30.0))
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new("Drop a video or audio file anywhere to play it")
                        .font(theme::sans(13.0))
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    let gap = ui.spacing().item_spacing.x;
                    ui.add_space(((width - 2.0 * 160.0 - gap) / 2.0).max(0.0));
                    let open = pill(ui, "Open File", true);
                    if open.on_hover_text(shortcut("O")).clicked() {
                        actions.push(Action::OpenFileDialog);
                    }
                    let stream = pill(ui, "Open Stream", false);
                    if stream.on_hover_text(shortcut("U")).clicked() {
                        actions.push(Action::ShowNetworkDialog);
                    }
                });
            });
            if !recent.is_empty() {
                ui.add_space(34.0);
                recent_list::show(ui, recent, now_seconds(), actions);
            }
        });
    paint_backdrop(ctx, logo_center);
}
