pub mod action;
mod audio_panel;
pub mod filters_panel;
mod hud;
pub mod icons;
mod inspector;
pub mod layer;
mod network_dialog;
mod playlist;
mod scrubber;
mod subtitle_panel;
pub mod theme;
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

#[derive(Default)]
pub struct Interface {
    playlist_open: bool,
    filters_open: bool,
    inspector_open: bool,
    filter_tab: filters_panel::Tab,
    network: network_dialog::NetworkDialog,
    toasts: toast::Toasts,
    last_activity: f64,
    hud_visible: bool,
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
        self.playlist_open
    }

    pub fn wants_cursor_hidden(&self) -> bool {
        !self.hud_visible
    }

    pub fn has_overlay(&self) -> bool {
        self.playlist_open || self.filters_open || self.network.is_open()
    }

    pub fn apply(&mut self, action: &Action) -> bool {
        match action {
            Action::TogglePlaylist => {
                self.playlist_open = !self.playlist_open;
                self.filters_open &= !self.playlist_open;
            }
            Action::ToggleFilters => {
                self.filters_open = !self.filters_open;
                self.playlist_open &= !self.filters_open;
            }
            Action::ShowFilters(tab) => {
                self.filters_open = !(self.filters_open && self.filter_tab == *tab);
                self.filter_tab = *tab;
                self.playlist_open &= !self.filters_open;
            }
            Action::ToggleInspector => self.inspector_open = !self.inspector_open,
            Action::ShowNetworkDialog => self.network.open(),
            Action::Dismiss if self.has_overlay() => {
                self.network.close();
                self.playlist_open = false;
                self.filters_open = false;
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
            welcome::show(&ctx, actions);
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
            && (!idle || player.is_paused() || self.has_overlay() || view.displayed.is_none());
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
            playlist: self.playlist_open,
            filters: self.filters_open,
            inspector: self.inspector_open,
        };
        let hud_rect = hud::show(&ctx, view, opacity, &toggles, actions);
        if let (Some(rect), Some(pointer)) = (hud_rect, ctx.pointer_hover_pos())
            && rect.contains(pointer)
        {
            self.last_activity = now;
        }
        self.hud_visible = wanted || opacity > 0.0 || !player.has_session();

        let playlist = ctx.animate_bool(Id::unique("playlist drawer state"), self.playlist_open);
        playlist::show(&ctx, view, playlist, actions);
        let filters = ctx.animate_bool(Id::unique("filters drawer state"), self.filters_open);
        filters_panel::show(&ctx, view, filters, &mut self.filter_tab, actions);
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
