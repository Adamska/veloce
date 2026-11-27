mod files;
mod frame;
mod icon;
mod input;
pub mod logging;
#[cfg(target_os = "macos")]
mod macos;
mod options;
mod viewport;

use std::{ffi::OsString, path::PathBuf, time::Instant};

use anyhow::Result;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::WindowId,
};

use crate::engine::ffmpeg_source;
use options::Options;
use viewport::Viewport;

#[derive(Debug)]
pub enum AppEvent {
    Redraw,
    Open(Vec<String>),
}

pub fn run(arguments: Vec<OsString>) -> Result<()> {
    let options = Options::parse(arguments)?;
    if options.show_help {
        println!("{}", options::USAGE);
        return Ok(());
    }
    if options.show_version {
        println!("veloce {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut log = logging::init(files::log_dir());
    let ran = play(options, &mut log);
    if let Err(error) = &ran {
        log::error!("{error:#}");
    }
    log.finish();
    ran
}

fn play(options: Options, log: &mut logging::LogSession) -> Result<()> {
    ffmpeg_source::initialize()?;
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    #[cfg(target_os = "macos")]
    macos::install_document_handler(event_loop.create_proxy());
    let mut app = App {
        options,
        waker: event_loop.create_proxy(),
        viewport: None,
        modifiers: ModifiersState::empty(),
        log,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App<'a> {
    options: Options,
    waker: EventLoopProxy<AppEvent>,
    viewport: Option<Viewport>,
    modifiers: ModifiersState,
    log: &'a mut logging::LogSession,
}

impl ApplicationHandler<AppEvent> for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.viewport.is_some() {
            return;
        }
        match Viewport::new(event_loop, &self.options, self.waker.clone()) {
            Ok(mut viewport) => {
                if self.log.crashed_before() {
                    let reveal = input::shortcut("Shift+L");
                    viewport.player.warn(format!(
                        "Veloce did not close properly last time, the log has details ({reveal})"
                    ));
                }
                self.viewport = Some(viewport);
            }
            Err(error) => {
                log::error!("failed to create the main window: {error:#}");
                event_loop.exit();
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(viewport) = self.viewport.as_mut() {
            viewport.player.save_state();
        }
        self.log.finish();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match (event, self.viewport.as_mut()) {
            (AppEvent::Open(paths), Some(viewport)) => {
                viewport
                    .dropped
                    .extend(paths.into_iter().map(PathBuf::from));
                viewport.window.focus_window();
                viewport.window.request_redraw();
            }
            (AppEvent::Open(paths), None) => self.options.locations.extend(paths),
            (AppEvent::Redraw, Some(viewport)) => viewport.window.request_redraw(),
            (AppEvent::Redraw, None) => {}
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(viewport) = self.viewport.as_mut() else {
            return;
        };
        let response = viewport.egui.on_window_event(&viewport.window, &event);
        if response.repaint {
            viewport.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                viewport.gpu.resize(size.width, size.height);
                viewport.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !viewport.egui.context().egui_wants_keyboard_input() =>
            {
                if let Some(action) = input::map(&event.logical_key, self.modifiers, event.repeat) {
                    viewport.dispatch(action, event_loop);
                    viewport.window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                viewport.dropped.push(path);
                viewport.window.request_redraw();
            }
            WindowEvent::RedrawRequested => viewport.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(viewport) = self.viewport.as_ref() else {
            return;
        };
        match viewport.next_repaint() {
            Some(deadline) if deadline <= Instant::now() => {
                event_loop.set_control_flow(ControlFlow::Wait);
                viewport.window.request_redraw();
            }
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}
