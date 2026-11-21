pub mod action;
mod audio_panel;
pub mod filters_panel;
mod hud;
pub mod icons;
mod inspector;
pub mod layer;
pub mod logo;
mod network_dialog;
mod playlist;
mod recent_list;
mod scrubber;
mod speed_control;
mod subtitle_panel;
pub mod theme;
mod title;
mod toast;
mod welcome;
pub mod widgets;

pub use action::Action;
pub use layer::{EguiFrame, EguiLayer};

use egui::{Id, LayerId, Order};

use crate::{
    media::VideoFrame,
    player::{Notice, Player},
    subtitles,
};

const HIDE_AFTER_SECONDS: f64 = 2.6;

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats<'a> {
    pub adapter: &'a str,
    pub fps: f32,
    pub frame_millis: f32,
    pub uploads: u64,
}

pub struct ViewContext<'a> {
    pub player: &'a Player,
    pub displayed: Option<&'a VideoFrame>,
    pub video_rect: Option<egui::Rect>,
    pub stats: RenderStats<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Playlist,
    Filters,
    Subtitles,
}

#[derive(Default)]
pub struct Interface {
    panel: Option<Panel>,
    more_controls: bool,
    inspector_open: bool,
    filter_tab: filters_panel::Tab,
    network: network_dialog::NetworkDialog,
    toasts: toast::Toasts,
    last_activity: f64,
    hud_visible: bool,
    logo: Option<egui::TextureHandle>,
}

impl Interface {
    pub fn notify(&mut self, notices: Vec<Notice>, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        for notice in notices {
            self.toasts.push(notice, now);
        }
    }

    pub fn with_inspector(inspector_open: bool) -> Self {
        Self {
            inspector_open,
            ..Self::default()
        }
    }

    pub fn playlist_open(&self) -> bool {
        self.panel == Some(Panel::Playlist)
    }

    pub fn wants_cursor_hidden(&self) -> bool {
        !self.hud_visible
    }

    pub fn has_overlay(&self) -> bool {
        self.panel.is_some() || self.network.is_open()
    }

    fn toggle(&mut self, panel: Panel) {
        self.panel = (self.panel != Some(panel)).then_some(panel);
    }

    pub fn apply(&mut self, action: &Action) -> bool {
        match action {
            Action::TogglePlaylist => self.toggle(Panel::Playlist),
            Action::ToggleFilters => self.toggle(Panel::Filters),
            Action::ToggleSubtitles => self.toggle(Panel::Subtitles),
            Action::ShowFilters(tab) => {
                let showing = self.panel == Some(Panel::Filters) && self.filter_tab == *tab;
                self.panel = (!showing).then_some(Panel::Filters);
                self.filter_tab = *tab;
            }
            Action::ToggleMoreControls => self.more_controls = !self.more_controls,
            Action::ToggleInspector => self.inspector_open = !self.inspector_open,
            Action::ShowNetworkDialog => self.network.open(),
            Action::Dismiss if self.has_overlay() => {
                self.network.close();
                self.panel = None;
            }
            _ => return false,
        }
        true
    }

    pub fn show(&mut self, ui: &mut egui::Ui, view: &ViewContext<'_>, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let player = view.player;
        let now = ctx.input(|input| input.time);
        let pointer_active = ctx.input(|input| {
            input.pointer.is_moving() || input.pointer.any_down() || input.pointer.any_released()
        });
        if pointer_active {
            self.last_activity = now;
        }

        let double_clicked = ctx.input(|input| {
            input
                .pointer
                .button_double_clicked(egui::PointerButton::Primary)
        });
        if double_clicked && player.has_session() && !ctx.is_pointer_over_egui() {
            actions.push(Action::ToggleFullscreen);
        }

        if !player.has_session() {
            if self.logo.is_none() {
                self.logo = logo::texture(&ctx);
            }
            let recent = player.recent().available();
            welcome::show(&ctx, self.logo.as_ref(), &recent, actions);
        } else if view.displayed.is_none() {
            let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::unique("status")));
            painter.text(
                ctx.content_rect().center(),
                egui::Align2::CENTER_CENTER,
                "Opening media",
                theme::mono(12.0),
                theme::TEXT_DIM,
            );
        }

        let idle = now - self.last_activity > HIDE_AFTER_SECONDS;
        let wanted = player.has_session()
            && (!idle
                || player.is_paused()
                || self.has_overlay()
                || egui::Popup::is_any_open(&ctx)
                || view.displayed.is_none());
        let opacity = ctx.animate_bool(Id::unique("hud visibility"), wanted);
        if let Some(area) = view.video_rect {
            let painter =
                ctx.layer_painter(LayerId::new(Order::Background, Id::unique("subtitles")));
            let lines = player.subtitles.lines_at(player.position());
            let clearance = (area.bottom() - ctx.content_rect().bottom() + widgets::HUD_RESERVE)
                .max(0.0)
                * opacity;
            let area = area.with_max_y(area.bottom() - clearance);
            subtitles::renderer::paint(&painter, area, &lines, &player.subtitles.style);
        }
        let toggles = hud::Toggles {
            playlist: self.panel == Some(Panel::Playlist),
            filters: self.panel == Some(Panel::Filters),
            subtitles: self.panel == Some(Panel::Subtitles),
            inspector: self.inspector_open,
            more: self.more_controls,
        };
        let hud_rect = hud::show(&ctx, view, opacity, &toggles, actions);
        title::show(&ctx, view, opacity);
        if let (Some(rect), Some(pointer)) = (hud_rect, ctx.pointer_hover_pos())
            && rect.contains(pointer)
        {
            self.last_activity = now;
        }
        self.hud_visible = wanted || opacity > 0.0 || !player.has_session();

        let openness = |name: &'static str, panel: Panel| {
            ctx.animate_bool(Id::unique(name), self.panel == Some(panel))
        };
        playlist::show(
            &ctx,
            view,
            openness("playlist drawer state", Panel::Playlist),
            actions,
        );
        let filters = openness("filters drawer state", Panel::Filters);
        let subtitles = openness("subtitle window state", Panel::Subtitles);
        filters_panel::show(&ctx, view, filters, &mut self.filter_tab, actions);
        let screen = ctx.content_rect();
        let anchor = egui::pos2(
            hud_rect.map_or(screen.right() - 24.0, |rect| rect.right()),
            screen.bottom() - widgets::HUD_RESERVE,
        );
        subtitle_panel::window(&ctx, view, subtitles, anchor, actions);
        if self.inspector_open {
            inspector::show(&ctx, view);
        }
        self.network.show(&ctx, actions);
        self.toasts.show(&ctx, now);

        if player.has_session() && !idle && !player.is_paused() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(HIDE_AFTER_SECONDS));
        }
    }
}
