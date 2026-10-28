//! Runs a cappy [`Ui`] in a winit window. The event loop sleeps until input arrives or a signal
//! changes, and each redraw repaints only the damaged regions.
//!
//! ```no_run
//! use cappy_core::{Theme, Ui, div, text};
//! use cappy_winit::{WindowOptions, run};
//!
//! let ui = Ui::new(div().class("app").child(text("Hello")), Theme::light());
//! run(WindowOptions::new("Hello"), ui).expect("app failed");
//! ```

mod translate;

use std::fmt;
use std::sync::Arc;

use cappy_core::{CursorIcon, InputEvent, Ui};
use cappy_render::{SurfaceError, WindowHost};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

pub use cappy_render::WindowOptions;

const DEBUG_DAMAGE_KEY: KeyCode = KeyCode::F12;

#[derive(Debug)]
pub enum RunError {
    EventLoop(winit::error::EventLoopError),
    CreateWindow(winit::error::OsError),
    Surface(SurfaceError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoop(error) => write!(f, "winit event loop failed: {error}"),
            Self::CreateWindow(error) => write!(f, "failed to create the window: {error}"),
            Self::Surface(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EventLoop(error) => Some(error),
            Self::CreateWindow(error) => Some(error),
            Self::Surface(error) => Some(error),
        }
    }
}

/// Opens a window showing `ui` and runs until it is closed.
pub fn run(options: WindowOptions, ui: Ui) -> Result<(), RunError> {
    let event_loop = EventLoop::<Wake>::with_user_event()
        .build()
        .map_err(RunError::EventLoop)?;
    let mut app = App {
        options,
        proxy: event_loop.create_proxy(),
        pending_ui: Some(ui),
        running: None,
        error: None,
    };
    event_loop.run_app(&mut app).map_err(RunError::EventLoop)?;
    app.error.map_or(Ok(()), Err)
}

/// Sent by the UI's waker when a signal changes outside of input handling.
#[derive(Debug)]
struct Wake;

struct Running {
    // Declared before `window` so the surface is dropped before the window it renders to.
    host: WindowHost,
    window: Arc<Window>,
}

struct App {
    options: WindowOptions,
    proxy: EventLoopProxy<Wake>,
    pending_ui: Option<Ui>,
    running: Option<Running>,
    error: Option<RunError>,
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop, ui: Ui) -> Result<Running, RunError> {
        let (width, height) = self.options.logical_size;
        let attributes = Window::default_attributes()
            .with_title(self.options.title.clone())
            .with_inner_size(LogicalSize::new(width, height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(RunError::CreateWindow)?,
        );
        let size = window.inner_size();
        let mut host = pollster::block_on(WindowHost::new(
            Arc::clone(&window),
            (size.width, size.height),
            ui,
        ))
        .map_err(RunError::Surface)?;
        host.set_debug_damage(self.options.debug_damage);
        let proxy = self.proxy.clone();
        host.set_waker(move || {
            if proxy.send_event(Wake).is_err() {
                log::debug!("wake ignored: the event loop has already exited");
            }
        });
        window.request_redraw();
        Ok(Running { host, window })
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: RunError) {
        log::error!("{error}");
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<Wake> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(ui) = self.pending_ui.take() else {
            return;
        };
        match self.start(event_loop, ui) {
            Ok(running) => self.running = Some(running),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: Wake) {
        if let Some(running) = &self.running {
            running.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if event == WindowEvent::RedrawRequested {
            let outcome = self.running.as_mut().map_or(Ok(()), redraw);
            if let Err(error) = outcome {
                self.fail(event_loop, error);
            }
            return;
        }
        let Some(running) = &mut self.running else {
            return;
        };
        let window = Arc::clone(&running.window);
        let input = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                None
            }
            WindowEvent::Resized(size) => {
                running.host.resize((size.width, size.height));
                window.request_redraw();
                None
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                window.request_redraw();
                None
            }
            WindowEvent::CursorMoved { position, .. } => {
                Some(translate::pointer_moved(position, window.scale_factor()))
            }
            WindowEvent::CursorLeft { .. } => Some(InputEvent::PointerLeft),
            WindowEvent::MouseInput { state, button, .. } => {
                translate::pointer_button(button, state)
            }
            WindowEvent::KeyboardInput { event, .. } if is_debug_toggle(&event) => {
                running.host.toggle_debug_damage();
                window.request_redraw();
                None
            }
            _ => None,
        };
        if let Some(input) = input {
            let response = running.host.handle_event(input);
            apply_cursor(&window, response.cursor);
            if response.redraw {
                window.request_redraw();
            }
        }
    }
}

fn apply_cursor(window: &Window, cursor: Option<CursorIcon>) {
    if let Some(cursor) = cursor {
        window.set_cursor(translate::cursor_icon(cursor));
    }
}

fn is_debug_toggle(event: &KeyEvent) -> bool {
    event.state == ElementState::Pressed
        && !event.repeat
        && event.physical_key == PhysicalKey::Code(DEBUG_DAMAGE_KEY)
}

fn redraw(running: &mut Running) -> Result<(), RunError> {
    let size = running.window.inner_size();
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    let result = running
        .host
        .redraw((size.width, size.height), running.window.scale_factor())
        .map_err(RunError::Surface)?;
    apply_cursor(&running.window, result.cursor);
    if result.needs_retry() {
        running.window.request_redraw();
    }
    Ok(())
}
