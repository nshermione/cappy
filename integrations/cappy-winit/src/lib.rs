//! Runs a cappy [`Ui`] in a winit window. The event loop sleeps until input arrives or a signal
//! changes, and each redraw repaints only the damaged regions. A live resize follows the pointer.
//! Presents stay on the resize interval, then one more frame is presented when the size settles.
//! The window stays hidden until that first frame has been presented.
//!
//! ```no_run
//! use cappy_core::{Application, Theme, Ui, div, text};
//! use cappy_winit::{Run, WindowOptions};
//!
//! let application = Application::new();
//! let ui = Ui::new(
//!     application.context(),
//!     div().class("app").child(text("Hello")),
//!     Theme::light(),
//! );
//! application
//!     .run(WindowOptions::new("Hello"), ui)
//!     .expect("app failed");
//! ```
//!
//! Features pick the renderer backends compiled in: `gpu` (wgpu, default) and `cpu`
//! (tiny-skia + softbuffer, far less memory). With both, [`WindowOptions::with_renderer`] or the
//! `CAPPY_RENDERER=gpu|cpu` environment variable chooses; otherwise the GPU is preferred.

mod renderer;
mod translate;

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use cappy_core::{Application, CursorIcon, InputEvent, Ui};
use cappy_host::{
    PresentOutcome, RendererError, RendererSelectionError, WindowHost, select_renderer,
};
use cappy_text::FontContext;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(target_os = "windows")]
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowAttributes, WindowId};

pub use cappy_host::{FontOptions, RendererKind, WindowOptions};
pub use renderer::AVAILABLE_RENDERERS;

const DEBUG_DAMAGE_KEY: KeyCode = KeyCode::F12;

fn window_attributes(options: &WindowOptions, renderer: RendererKind) -> WindowAttributes {
    let (width, height) = options.logical_size;
    let attributes = Window::default_attributes()
        .with_title(options.title.clone())
        .with_inner_size(LogicalSize::new(width, height))
        // Shown after the first presented frame. A hidden window is not transparent on screen.
        .with_visible(false);
    without_redirection_bitmap(attributes, renderer)
}

/// DWM keeps a bitmap of the window and scales it on the GPU for the whole drag.
/// The GPU swapchain is the window content, so that bitmap is dropped.
/// The CPU backend blits through GDI and still needs it.
#[cfg(target_os = "windows")]
fn without_redirection_bitmap(
    attributes: WindowAttributes,
    renderer: RendererKind,
) -> WindowAttributes {
    if renderer == RendererKind::Gpu {
        attributes.with_no_redirection_bitmap(true)
    } else {
        attributes
    }
}

#[cfg(not(target_os = "windows"))]
fn without_redirection_bitmap(
    attributes: WindowAttributes,
    _renderer: RendererKind,
) -> WindowAttributes {
    attributes
}

#[derive(Debug)]
pub enum RunError {
    /// No usable renderer: an invalid `CAPPY_RENDERER`, or a backend that was not compiled in.
    SelectRenderer(RendererSelectionError),
    EventLoop(winit::error::EventLoopError),
    CreateWindow(winit::error::OsError),
    Renderer(RendererError),
    /// `ui` was built from a different [`Application`] than the one [`Run::run`] was called on.
    ContextMismatch,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectRenderer(error) => write!(f, "cannot choose a renderer: {error}"),
            Self::EventLoop(error) => write!(f, "winit event loop failed: {error}"),
            Self::CreateWindow(error) => write!(f, "failed to create the window: {error}"),
            Self::Renderer(error) => write!(f, "{error}"),
            Self::ContextMismatch => {
                write!(
                    f,
                    "ui was built with a different context than the application"
                )
            }
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SelectRenderer(error) => Some(error),
            Self::EventLoop(error) => Some(error),
            Self::CreateWindow(error) => Some(error),
            Self::Renderer(error) => Some(error),
            Self::ContextMismatch => None,
        }
    }
}

/// Opens a window for an [`Application`] and runs until it is closed.
pub trait Run {
    fn run(&self, options: WindowOptions, ui: Ui) -> Result<(), RunError>;
}

impl Run for Application {
    fn run(&self, options: WindowOptions, ui: Ui) -> Result<(), RunError> {
        run_window(self, options, ui)
    }
}

fn run_window(application: &Application, options: WindowOptions, ui: Ui) -> Result<(), RunError> {
    if !ui.shares_context(application.context()) {
        return Err(RunError::ContextMismatch);
    }
    let renderer =
        select_renderer(options.renderer, AVAILABLE_RENDERERS).map_err(RunError::SelectRenderer)?;
    let event_loop = EventLoop::<Wake>::with_user_event()
        .build()
        .map_err(RunError::EventLoop)?;
    let mut app = WinitHost {
        options,
        renderer,
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
    // Declared before `window` so the renderer is dropped before the window it renders to.
    host: WindowHost,
    window: Arc<Window>,
    /// When set, redraw then so a resize presents on its cadence or at the resting size.
    settle_at: Option<Instant>,
    /// Set once the first presented frame has been followed by [`Window::set_visible`].
    shown: bool,
}

struct WinitHost {
    options: WindowOptions,
    renderer: RendererKind,
    proxy: EventLoopProxy<Wake>,
    pending_ui: Option<Ui>,
    running: Option<Running>,
    error: Option<RunError>,
}

impl WinitHost {
    fn start(&mut self, event_loop: &ActiveEventLoop, ui: Ui) -> Result<Running, RunError> {
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start before window");
        let attributes = window_attributes(&self.options, self.renderer);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(RunError::CreateWindow)?,
        );
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after window");
        let fonts = FontContext::load(&self.options.fonts);
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after fonts");
        let renderer =
            renderer::open_renderer(self.renderer, &window, &fonts).map_err(RunError::Renderer)?;
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after renderer");
        let mut host = WindowHost::new(renderer, &fonts, ui);
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after host");
        host.set_debug_damage(self.options.debug_damage);
        let proxy = self.proxy.clone();
        host.set_waker(move || {
            if proxy.send_event(Wake).is_err() {
                log::debug!("wake ignored: the event loop has already exited");
            }
        });
        let mut running = Running {
            host,
            window,
            settle_at: None,
            shown: false,
        };
        // A hidden window does not receive WM_PAINT, so the first frame cannot wait for
        // RedrawRequested. Present it here, then show the window with that frame in place.
        redraw(&mut running)?;
        Ok(running)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: RunError) {
        log::error!("{error}");
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<Wake> for WinitHost {
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(running) = &self.running else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        match running.settle_at {
            Some(deadline) if Instant::now() >= deadline => {
                running.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
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
            WindowEvent::MouseWheel { delta, .. } => {
                Some(translate::mouse_wheel(delta, window.scale_factor()))
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
        running.settle_at = None;
        if !running.shown {
            // No client area yet. Showing is what lets a later resize deliver a paint.
            running.window.set_visible(true);
            running.shown = true;
        }
        return Ok(());
    }
    let result = running
        .host
        .redraw((size.width, size.height), running.window.scale_factor())
        .map_err(RunError::Renderer)?;
    apply_cursor(&running.window, result.cursor);
    running.settle_at = result.settle_at;
    if !running.shown {
        match result.present {
            PresentOutcome::Presented { .. } => {
                running.window.set_visible(true);
                running.shown = true;
                log::info!("showed the window after the first presented frame");
            }
            // A hidden window receives no WM_PAINT, and some backends report it occluded.
            // Show it and ask for another frame; the device is already open, so this is not
            // the startup wait.
            PresentOutcome::Occluded | PresentOutcome::Retry => {
                running.window.set_visible(true);
                running.shown = true;
                running.window.request_redraw();
            }
            PresentOutcome::Deferred => {}
        }
    }
    if result.needs_retry() {
        running.window.request_redraw();
    }
    Ok(())
}
