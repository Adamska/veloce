use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use winit::{
    dpi::LogicalSize,
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::{Fullscreen, Window},
};

use super::{AppEvent, files, icon, logging, options::Options};
use crate::{
    gpu::{GpuContext, VideoPipeline, snapshot},
    player::{Command, Player, RecentFiles, Resume, Settings},
    ui::{Action, EguiLayer, Interface},
};

const FALLBACK_REFRESH: Duration = Duration::from_micros(16_667);

pub struct Viewport {
    pub window: Arc<Window>,
    pub gpu: GpuContext,
    pub egui: EguiLayer,
    pub dropped: Vec<PathBuf>,
    pub(super) video: VideoPipeline,
    snapshot_pipeline: Option<VideoPipeline>,
    pub(super) snapshots: (Sender<Result<PathBuf>>, Receiver<Result<PathBuf>>),
    waker: EventLoopProxy<AppEvent>,
    pub(super) player: Player,
    pub(super) interface: Interface,
    pub(super) next_repaint: Option<Instant>,
    pub(super) last_frame: Instant,
    pub(super) frame_millis: f32,
    title: String,
    cursor_visible: bool,
}

fn window_attributes() -> winit::window::WindowAttributes {
    let attributes = Window::default_attributes()
        .with_title("Veloce")
        .with_window_icon(icon::window_icon())
        .with_inner_size(LogicalSize::new(1280.0, 720.0))
        .with_min_inner_size(LogicalSize::new(720.0, 405.0));
    #[cfg(target_os = "macos")]
    let attributes = {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
    };
    attributes
}

impl Viewport {
    pub fn new(
        event_loop: &ActiveEventLoop,
        options: &Options,
        waker: EventLoopProxy<AppEvent>,
    ) -> Result<Self> {
        let window = Arc::new(event_loop.create_window(window_attributes())?);
        let gpu = GpuContext::new(window.clone())?;
        let egui = EguiLayer::new(&window, &gpu.device, gpu.format());
        let video = VideoPipeline::new(&gpu.device, gpu.format());
        log::info!("GPU adapter: {}", gpu.adapter_name);
        let mut player = Player::new(gpu.capabilities, options.cache_limits());
        let (device, rate) = player.audio_device();
        log::info!("audio output: {device} at {rate} Hz");
        if let Some(directory) = files::data_dir() {
            let (settings, saved) = Settings::persistent(directory.join("settings"));
            player.set_settings(settings, saved);
            player.set_resume(Resume::persistent(directory.join("resume")));
            let waker = waker.clone();
            player.set_recent(RecentFiles::persistent(
                directory.join("recent"),
                move || {
                    let _ = waker.send_event(AppEvent::Redraw);
                },
            ));
        }
        if options.mute {
            player.execute(Command::ToggleMute);
            player.take_notices();
        }
        if options.demo {
            player.execute(Command::OpenDemo);
        } else if !options.locations.is_empty() {
            player.execute(Command::Open(options.locations.clone()));
        } else {
            player.restore_session();
        }
        if options.paused && player.has_session() && !player.is_paused() {
            player.execute(Command::TogglePause);
        }
        Ok(Self {
            window,
            gpu,
            egui,
            dropped: Vec::new(),
            video,
            snapshot_pipeline: None,
            snapshots: crossbeam_channel::unbounded(),
            waker,
            player,
            interface: Interface::with_inspector(options.inspector),
            next_repaint: Some(Instant::now()),
            last_frame: Instant::now(),
            frame_millis: 0.0,
            title: String::new(),
            cursor_visible: true,
        })
    }

    pub fn next_repaint(&self) -> Option<Instant> {
        self.next_repaint
    }

    pub(super) fn refresh_interval(&self) -> Duration {
        self.window
            .current_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz())
            .filter(|millihertz| *millihertz > 0)
            .map_or(FALLBACK_REFRESH, |millihertz| {
                Duration::from_secs_f64(1_000.0 / f64::from(millihertz))
            })
    }

    pub(super) fn handle_drops(&mut self) {
        if self.dropped.is_empty() {
            return;
        }
        let dropped = files::classify(std::mem::take(&mut self.dropped));
        if !dropped.media.is_empty() {
            let queue = self.interface.playlist_open() && self.player.has_session();
            self.player.execute(if queue {
                Command::Enqueue(dropped.media)
            } else {
                Command::Open(dropped.media)
            });
        }
        for path in dropped.subtitles {
            self.player.execute(Command::LoadSubtitles(path));
        }
    }

    fn take_snapshot(&mut self) {
        let Some(frame) = self.video.displayed().cloned() else {
            self.player.notify("Nothing to capture yet");
            return;
        };
        let pipeline = self
            .snapshot_pipeline
            .get_or_insert_with(|| VideoPipeline::new(&self.gpu.device, snapshot::SNAPSHOT_FORMAT));
        let captured = snapshot::capture(
            &self.gpu.device,
            &self.gpu.queue,
            pipeline,
            &frame,
            &self.player.adjustments,
        );
        match captured {
            Ok(image) => {
                let title = self
                    .player
                    .info()
                    .map_or("snapshot", |info| info.title.as_str());
                let path = files::snapshot_path(title, frame.pts);
                let (results, waker) = (self.snapshots.0.clone(), self.waker.clone());
                std::thread::spawn(move || {
                    let _ = results.send(image.save_png(&path).map(|()| path));
                    let _ = waker.send_event(AppEvent::Redraw);
                });
            }
            Err(error) => self
                .player
                .report_error(format!("Snapshot failed: {error:#}")),
        }
    }

    pub fn dispatch(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Player(command) => self.player.execute(command),
            Action::OpenFileDialog => {
                if let Some(media) = files::pick_media() {
                    self.player.execute(Command::Open(media));
                }
            }
            Action::AddFilesDialog => {
                if let Some(media) = files::pick_media() {
                    self.player.execute(Command::Enqueue(media));
                }
            }
            Action::OpenSubtitleDialog => {
                if let Some(path) = files::pick_subtitles() {
                    self.player.execute(Command::LoadSubtitles(path));
                }
            }
            Action::ToggleFullscreen => {
                let target = match self.window.fullscreen() {
                    Some(_) => None,
                    None => Some(Fullscreen::Borderless(None)),
                };
                self.window.set_fullscreen(target);
            }
            Action::Snapshot => self.take_snapshot(),
            Action::RevealLog => match logging::log_path() {
                Some(path) => {
                    if let Err(error) = files::reveal(&path) {
                        self.player
                            .report_error(format!("Cannot show {}: {error}", path.display()));
                    }
                }
                None => self.player.notify("No log directory on this system"),
            },
            Action::Quit => event_loop.exit(),
            Action::Dismiss => {
                if !self.interface.apply(&Action::Dismiss) {
                    self.window.set_fullscreen(None);
                }
            }
            other => {
                self.interface.apply(&other);
            }
        }
    }

    pub(super) fn sync_window(&mut self) {
        let title = match self.player.info() {
            Some(info) => format!("{} - Veloce", info.title),
            None => "Veloce".to_owned(),
        };
        if title != self.title {
            self.window.set_title(&title);
            self.title = title;
        }
        let visible = !self.interface.wants_cursor_hidden();
        if visible != self.cursor_visible {
            self.window.set_cursor_visible(visible);
            self.cursor_visible = visible;
        }
    }
}
