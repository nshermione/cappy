//! Runs a cappy [`Ui`] in a winit window. The event loop sleeps until input arrives or a signal
//! changes, and each redraw repaints only the damaged regions. A live resize follows the pointer.
//! Presents stay on the resize interval, then one more frame is presented when the size settles.
//! The window stays hidden until that first frame has been presented. On the web the canvas fills
//! the page and is shown immediately, because a hidden canvas has no size for wgpu to draw to.
//! Animation there waits for the browser's frame, and `run_app` is replaced by `spawn_app` so the
//! call returns while the browser keeps the loop alive.
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
//! The web target only builds the GPU backend.

mod renderer;
mod translate;

use cappy_core::time::Instant;
use std::fmt;
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

use cappy_core::{Application, CursorIcon, InputEvent, Ui};
use cappy_host::{
    PresentOutcome, RendererError, RendererSelectionError, WindowHost, WindowRenderer,
    select_renderer,
};
use cappy_text::FontContext;
use winit::application::ApplicationHandler;
#[cfg(not(target_arch = "wasm32"))]
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
    // Native windows stay hidden until the first presented frame. On the web a hidden canvas
    // reports a zero size, so wgpu cannot create a surface; the page shows it immediately.
    // The page CSS sizes that canvas. An inner size would pin it to a fixed pixel box.
    let attributes = Window::default_attributes()
        .with_title(options.title.clone())
        .with_visible(cfg!(target_arch = "wasm32"));
    let attributes = with_logical_size(attributes, options.logical_size);
    web_canvas(without_redirection_bitmap(attributes, renderer))
}

#[cfg(not(target_arch = "wasm32"))]
fn with_logical_size(attributes: WindowAttributes, size: (f64, f64)) -> WindowAttributes {
    attributes.with_inner_size(LogicalSize::new(size.0, size.1))
}

#[cfg(target_arch = "wasm32")]
fn with_logical_size(attributes: WindowAttributes, _size: (f64, f64)) -> WindowAttributes {
    attributes
}

#[cfg(target_arch = "wasm32")]
fn web_canvas(attributes: WindowAttributes) -> WindowAttributes {
    use winit::platform::web::WindowAttributesExtWebSys;
    attributes.with_append(true)
}

#[cfg(not(target_arch = "wasm32"))]
fn web_canvas(attributes: WindowAttributes) -> WindowAttributes {
    attributes
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
///
/// On the web this returns once the browser event loop is running. A failure while opening the
/// GPU surface is logged from that loop; it is not returned here.
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
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut app = WinitHost {
        options,
        renderer,
        proxy: event_loop.create_proxy(),
        pending_ui: Some(ui),
        running: None,
        error: None,
        #[cfg(target_arch = "wasm32")]
        opening: None,
        // Last non-zero canvas size. The first resize often arrives before the GPU device
        // exists, and a zero size would configure a 1×1 surface that then stretches over the page.
        #[cfg(target_arch = "wasm32")]
        surface_size: (0, 0),
        #[cfg(target_arch = "wasm32")]
        ready: Rc::new(RefCell::new(None)),
    };
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        event_loop.run_app(&mut app).map_err(RunError::EventLoop)?;
        app.error.map_or(Ok(()), Err)
    }
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

/// Window and UI waiting for the async GPU surface. Only the web opens the device this way.
#[cfg(target_arch = "wasm32")]
struct Opening {
    window: Arc<Window>,
    fonts: FontContext,
    ui: Ui,
    /// Set once the GPU future has been spawned. A zero canvas size is not a surface yet.
    gpu_started: bool,
}

#[cfg(target_arch = "wasm32")]
type ReadyRenderer = Rc<RefCell<Option<Result<Box<dyn WindowRenderer>, RendererError>>>>;

struct WinitHost {
    options: WindowOptions,
    renderer: RendererKind,
    proxy: EventLoopProxy<Wake>,
    pending_ui: Option<Ui>,
    running: Option<Running>,
    error: Option<RunError>,
    #[cfg(target_arch = "wasm32")]
    opening: Option<Opening>,
    /// Filled by the GPU future, then taken on the next wake. The browser types inside the
    /// renderer are not `Send`, so this stays on the page thread instead of riding the proxy.
    #[cfg(target_arch = "wasm32")]
    ready: ReadyRenderer,
    /// Last non-zero physical size reported for the canvas.
    #[cfg(target_arch = "wasm32")]
    surface_size: (u32, u32),
}

impl WinitHost {
    fn create_window(&self, event_loop: &ActiveEventLoop) -> Result<Arc<Window>, RunError> {
        cappy_host::set_log_memory(self.options.log_memory);
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
        Ok(window)
    }

    fn finish(
        &self,
        window: Arc<Window>,
        fonts: FontContext,
        ui: Ui,
        renderer: Box<dyn WindowRenderer>,
    ) -> Running {
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after renderer");
        let mut host = WindowHost::new(renderer, &fonts, ui);
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after host");
        host.set_debug_damage(self.options.debug_damage);
        host.set_log_frames(self.options.log_frames);
        let proxy = self.proxy.clone();
        host.set_waker(move || {
            if proxy.send_event(Wake).is_err() {
                log::debug!("wake ignored: the event loop has already exited");
            }
        });
        Running {
            host,
            window,
            settle_at: None,
            shown: false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, event_loop: &ActiveEventLoop, ui: Ui) -> Result<Running, RunError> {
        let window = self.create_window(event_loop)?;
        let fonts = FontContext::load(&self.options.fonts);
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after fonts");
        let renderer =
            renderer::open_renderer(self.renderer, &window, &fonts).map_err(RunError::Renderer)?;
        let mut running = self.finish(window, fonts, ui, renderer);
        // A hidden window does not receive WM_PAINT, so the first frame cannot wait for
        // RedrawRequested. Present it here, then show the window with that frame in place.
        redraw(&mut running)?;
        Ok(running)
    }

    /// Creates the canvas, then opens the GPU device without blocking the page.
    #[cfg(target_arch = "wasm32")]
    fn begin(&mut self, event_loop: &ActiveEventLoop, ui: Ui) {
        if self.renderer != RendererKind::Gpu {
            self.fail(
                event_loop,
                RunError::Renderer(RendererError::new(
                    self.renderer,
                    RendererSelectionError::Unavailable(self.renderer),
                )),
            );
            return;
        }
        let window = match self.create_window(event_loop) {
            Ok(window) => window,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        let fonts = FontContext::load(&self.options.fonts);
        self.opening = Some(Opening {
            window,
            fonts,
            ui,
            gpu_started: false,
        });
        // `create_window` may already have delivered `Resized` before `opening` existed.
        self.open_surface();
    }

    /// Starts the GPU future once the canvas has a real size. Called again from later resizes.
    #[cfg(target_arch = "wasm32")]
    fn open_surface(&mut self) {
        let Some(opening) = &self.opening else {
            return;
        };
        if opening.gpu_started {
            return;
        }
        let size = if self.surface_size.0 > 0 && self.surface_size.1 > 0 {
            self.surface_size
        } else {
            let inner = opening.window.inner_size();
            (inner.width, inner.height)
        };
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        self.surface_size = size;
        let opening = self.opening.as_mut().expect("opening is present");
        opening.gpu_started = true;
        let ready = Rc::clone(&self.ready);
        let proxy = self.proxy.clone();
        let surface_window = Arc::clone(&opening.window);
        let surface_fonts = opening.fonts.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let outcome = renderer::open_gpu(surface_window, size, &surface_fonts).await;
            *ready.borrow_mut() = Some(outcome);
            if proxy.send_event(Wake).is_err() {
                log::debug!("wake ignored: the event loop has already exited");
            }
        });
    }

    /// Builds the host once the GPU future has stored a renderer.
    #[cfg(target_arch = "wasm32")]
    fn attach_renderer(&mut self, event_loop: &ActiveEventLoop) {
        let Some(outcome) = self.ready.borrow_mut().take() else {
            return;
        };
        let Some(opening) = self.opening.take() else {
            log::error!("gpu renderer became ready without a window");
            return;
        };
        match outcome {
            Ok(renderer) => {
                let mut running = self.finish(opening.window, opening.fonts, opening.ui, renderer);
                match redraw(&mut running) {
                    Ok(()) => self.running = Some(running),
                    Err(error) => self.fail(event_loop, error),
                }
            }
            Err(error) => self.fail(event_loop, RunError::Renderer(error)),
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: RunError) {
        log::error!("{error}");
        #[cfg(target_arch = "wasm32")]
        set_boot_status(&error.to_string());
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<Wake> for WinitHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(ui) = self.pending_ui.take() else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        match self.start(event_loop, ui) {
            Ok(running) => self.running = Some(running),
            Err(error) => self.fail(event_loop, error),
        }
        #[cfg(target_arch = "wasm32")]
        self.begin(event_loop, ui);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: Wake) {
        #[cfg(target_arch = "wasm32")]
        self.attach_renderer(event_loop);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = event_loop;
        if self.error.is_some() {
            return;
        }
        if let Some(running) = &self.running {
            running.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        self.attach_renderer(event_loop);
        if self.error.is_some() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.open_surface();
        let Some(running) = &self.running else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        schedule_animation(event_loop, running);
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
        #[cfg(target_arch = "wasm32")]
        if let WindowEvent::Resized(size) = &event {
            if size.width > 0 && size.height > 0 {
                self.surface_size = (size.width, size.height);
                self.open_surface();
            }
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

/// Asks for the next animation frame, or sleeps when nothing is moving.
///
/// Native waits until the host deadline, then asks winit to redraw. The browser already
/// delivers frames through `requestAnimationFrame`. A `WaitUntil` timer would wake the page
/// between those frames and record an update the display cannot show.
fn schedule_animation(event_loop: &ActiveEventLoop, running: &Running) {
    #[cfg(target_arch = "wasm32")]
    {
        if running.settle_at.is_some() {
            running.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
    #[cfg(not(target_arch = "wasm32"))]
    match running.settle_at {
        Some(deadline) if Instant::now() >= deadline => {
            running.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
        None => event_loop.set_control_flow(ControlFlow::Wait),
    }
}

#[cfg(target_arch = "wasm32")]
fn set_boot_status(message: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    if let Some(boot) = document.get_element_by_id("boot") {
        let _ = boot.set_attribute("style", "display:block;padding:16px;white-space:pre-wrap");
        boot.set_text_content(Some(message));
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
