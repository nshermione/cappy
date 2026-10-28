//! Runs a cappy [`Ui`] in a tao window (the windowing library behind Tauri). The event loop
//! sleeps until input arrives or a signal changes, and each redraw repaints only the damaged
//! regions.
//!
//! ```no_run
//! use cappy_core::{Theme, Ui, div, text};
//! use cappy_tao::{WindowOptions, run};
//!
//! let ui = Ui::new(div().class("app").child(text("Hello")), Theme::light());
//! run(WindowOptions::new("Hello"), ui).expect("app failed");
//! ```
//!
//! Features pick the renderer backends compiled in: `gpu` (wgpu, default) and `cpu`
//! (tiny-skia + softbuffer, far less memory). With both, [`WindowOptions::with_renderer`] or the
//! `CAPPY_RENDERER=gpu|cpu` environment variable chooses; otherwise the GPU is preferred.

mod renderer;
mod translate;

use std::fmt;
use std::sync::Arc;

use cappy_core::{CursorIcon, InputEvent, Ui};
use cappy_host::{RendererError, RendererSelectionError, WindowHost, select_renderer};
use cappy_text::FontContext;
use tao::dpi::LogicalSize;
use tao::event::{ElementState, Event, KeyEvent, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
use tao::keyboard::KeyCode;
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::{Window, WindowBuilder};

pub use cappy_host::{RendererKind, WindowOptions};
pub use renderer::AVAILABLE_RENDERERS;

const DEBUG_DAMAGE_KEY: KeyCode = KeyCode::F12;

#[derive(Debug)]
pub enum RunError {
    /// No usable renderer: an invalid `CAPPY_RENDERER`, or a backend that was not compiled in.
    SelectRenderer(RendererSelectionError),
    CreateWindow(tao::error::OsError),
    Renderer(RendererError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectRenderer(error) => write!(f, "cannot choose a renderer: {error}"),
            Self::CreateWindow(error) => write!(f, "failed to create the window: {error}"),
            Self::Renderer(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SelectRenderer(error) => Some(error),
            Self::CreateWindow(error) => Some(error),
            Self::Renderer(error) => Some(error),
        }
    }
}

/// Sent by the UI's waker when a signal changes outside of input handling.
#[derive(Debug)]
struct Wake;

struct Running {
    // Declared before `window` so the renderer is dropped before the window it renders to.
    host: WindowHost,
    window: Arc<Window>,
}

/// Opens a window showing `ui` and runs until it is closed.
pub fn run(options: WindowOptions, ui: Ui) -> Result<(), RunError> {
    let mut event_loop = EventLoopBuilder::<Wake>::with_user_event().build();
    let mut running = start(&event_loop, &options, ui)?;
    let mut failure = None;
    event_loop.run_return(|event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Err(error) = handle(&mut running, event, control_flow) {
            log::error!("{error}");
            failure = Some(error);
            *control_flow = ControlFlow::Exit;
        }
    });
    failure.map_or(Ok(()), Err)
}

fn start(
    event_loop: &EventLoop<Wake>,
    options: &WindowOptions,
    ui: Ui,
) -> Result<Running, RunError> {
    let kind =
        select_renderer(options.renderer, AVAILABLE_RENDERERS).map_err(RunError::SelectRenderer)?;
    let (width, height) = options.logical_size;
    let window = WindowBuilder::new()
        .with_title(options.title.clone())
        .with_inner_size(LogicalSize::new(width, height))
        .build(event_loop)
        .map_err(RunError::CreateWindow)?;
    let window = Arc::new(window);
    let fonts = FontContext::from_system();
    let renderer = renderer::open_renderer(kind, &window, &fonts).map_err(RunError::Renderer)?;
    let mut host = WindowHost::new(renderer, &fonts, ui);
    host.set_debug_damage(options.debug_damage);
    let proxy = event_loop.create_proxy();
    host.set_waker(move || {
        if proxy.send_event(Wake).is_err() {
            log::debug!("wake ignored: the event loop has already exited");
        }
    });
    window.request_redraw();
    Ok(Running { host, window })
}

fn handle(
    running: &mut Running,
    event: Event<'_, Wake>,
    control_flow: &mut ControlFlow,
) -> Result<(), RunError> {
    match event {
        Event::WindowEvent { event, .. } => {
            if let Some(input) = window_input(running, event, control_flow) {
                let response = running.host.handle_event(input);
                apply_cursor(&running.window, response.cursor);
                if response.redraw {
                    running.window.request_redraw();
                }
            }
            Ok(())
        }
        Event::RedrawRequested(_) => redraw(running),
        Event::UserEvent(Wake) => {
            running.window.request_redraw();
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Handles window-level events and returns the pointer input to forward to the UI, if any.
fn window_input(
    running: &mut Running,
    event: WindowEvent<'_>,
    control_flow: &mut ControlFlow,
) -> Option<InputEvent> {
    let window = &running.window;
    match event {
        WindowEvent::CloseRequested => {
            *control_flow = ControlFlow::Exit;
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
        WindowEvent::MouseInput { state, button, .. } => translate::pointer_button(button, state),
        WindowEvent::KeyboardInput { event, .. } if is_debug_toggle(&event) => {
            running.host.toggle_debug_damage();
            window.request_redraw();
            None
        }
        _ => None,
    }
}

fn is_debug_toggle(event: &KeyEvent) -> bool {
    event.state == ElementState::Pressed && !event.repeat && event.physical_key == DEBUG_DAMAGE_KEY
}

fn redraw(running: &mut Running) -> Result<(), RunError> {
    let size = running.window.inner_size();
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    let result = running
        .host
        .redraw((size.width, size.height), running.window.scale_factor())
        .map_err(RunError::Renderer)?;
    apply_cursor(&running.window, result.cursor);
    if result.needs_retry() {
        running.window.request_redraw();
    }
    Ok(())
}

fn apply_cursor(window: &Window, cursor: Option<CursorIcon>) {
    if let Some(cursor) = cursor {
        window.set_cursor_icon(translate::cursor_icon(cursor));
    }
}
