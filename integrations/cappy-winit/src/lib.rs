//! Runs a cappy [`Ui`] in a winit window. The event loop sleeps until input arrives or a signal
//! changes, and each redraw repaints only the damaged regions. A live resize follows the pointer.
//! Presents stay on the resize interval, then one more frame is presented when the size settles.
//! A surface request from the UI opens another native window on this same loop. The web target
//! keeps a single canvas and drops those requests. The window stays hidden until that first
//! frame has been presented. On the web the canvas fills the page and is shown immediately,
//! because a hidden canvas has no size for wgpu to draw to.
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

#[cfg(target_arch = "wasm32")]
use cappy_core::time::Duration;
use cappy_core::time::Instant;
use std::fmt;
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use cappy_core::NoopClipboard;
use cappy_core::{Application, Context, CursorIcon, InputEvent, Modifiers, SurfaceId, Ui};
#[cfg(not(target_arch = "wasm32"))]
use cappy_core::{Element, SurfaceCommand, SurfaceOptions, Theme};
use cappy_host::{
    PresentOutcome, RendererError, RendererSelectionError, WindowHost, WindowRenderer,
    WindowSession, select_renderer,
};
use cappy_text::FontContext;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(target_os = "windows")]
use winit::platform::windows::{HWND, WindowAttributesExtWindows};
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
        context: application.context().clone(),
        pending_ui: Some(ui),
        session: WindowSession::new(),
        primary: None,
        popups: Vec::new(),
        #[cfg(not(target_arch = "wasm32"))]
        fonts: None,
        error: None,
        #[cfg(target_arch = "wasm32")]
        warned_surface: false,
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

/// Platform window beside a [`WindowHost`]. The host lives in [`WindowSession`], which is
/// dropped first so the renderer releases the surface before the window.
struct WindowChrome {
    window: Arc<Window>,
    /// When set, redraw then so a resize presents on its cadence or at the resting size.
    settle_at: Option<Instant>,
    /// Set once the first presented frame has been followed by [`Window::set_visible`].
    shown: bool,
    modifiers: Modifiers,
    /// Last caret rect sent to the OS. `None` means no input is focused.
    ime_caret: Option<cappy_core::Rect>,
}

enum WindowSlot {
    Primary,
    Popup(usize),
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
    context: Context,
    pending_ui: Option<Ui>,
    /// Dropped before [`Self::primary`] and [`Self::popups`] so each renderer releases its
    /// surface before the window.
    session: WindowSession<WindowHost>,
    primary: Option<WindowChrome>,
    popups: Vec<(SurfaceId, WindowChrome)>,
    #[cfg(not(target_arch = "wasm32"))]
    fonts: Option<FontContext>,
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
    /// One warning for surface requests the browser cannot open.
    #[cfg(target_arch = "wasm32")]
    warned_surface: bool,
}

impl WinitHost {
    fn create_window(
        &self,
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Result<Arc<Window>, RunError> {
        cappy_host::set_log_memory(self.options.log_memory);
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start before window");
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
        fonts: &FontContext,
        ui: Ui,
        renderer: Box<dyn WindowRenderer>,
    ) -> (WindowHost, WindowChrome) {
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
        install_clipboard(host.ui_mut());
        (
            host,
            WindowChrome {
                window,
                settle_at: None,
                shown: false,
                modifiers: Modifiers::default(),
                ime_caret: None,
            },
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, event_loop: &ActiveEventLoop, ui: Ui) -> Result<(), RunError> {
        let window =
            self.create_window(event_loop, window_attributes(&self.options, self.renderer))?;
        let fonts = FontContext::load(&self.options.fonts);
        self.fonts = Some(fonts.clone());
        #[cfg(debug_assertions)]
        cappy_host::memory_mark("start after fonts");
        let renderer =
            renderer::open_renderer(self.renderer, &window, &fonts).map_err(RunError::Renderer)?;
        let (mut host, mut chrome) = self.finish(window, &fonts, ui, renderer);
        // A hidden window does not receive WM_PAINT, so the first frame cannot wait for
        // RedrawRequested. Present it here, then show the window with that frame in place.
        redraw(&mut chrome, &mut host)?;
        self.session.set_primary(host);
        self.primary = Some(chrome);
        Ok(())
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
        let window =
            match self.create_window(event_loop, window_attributes(&self.options, self.renderer)) {
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
        self.open_gpu_surface();
    }

    /// Starts the GPU future once the canvas has a real size. Called again from later resizes.
    #[cfg(target_arch = "wasm32")]
    fn open_gpu_surface(&mut self) {
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
                let (mut host, mut chrome) =
                    self.finish(opening.window, &opening.fonts, opening.ui, renderer);
                match redraw(&mut chrome, &mut host) {
                    Ok(()) => {
                        self.session.set_primary(host);
                        self.primary = Some(chrome);
                    }
                    Err(error) => self.fail(event_loop, error),
                }
            }
            Err(error) => self.fail(event_loop, RunError::Renderer(error)),
        }
    }

    fn apply_surface_commands(&mut self, event_loop: &ActiveEventLoop) {
        if self.primary.is_none() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.discard_surface_commands();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            for command in self.context.drain_surfaces() {
                match command {
                    SurfaceCommand::Open { id, options, root } => {
                        if let Err(error) = self.open_popup(event_loop, id, options, root) {
                            self.fail(event_loop, error);
                            return;
                        }
                    }
                    SurfaceCommand::Close(id) => self.close_popup(id),
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn discard_surface_commands(&mut self) {
        if self.context.drain_surfaces().is_empty() || self.warned_surface {
            return;
        }
        self.warned_surface = true;
        log::warn!("popup windows stay closed in the browser");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_popup(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: SurfaceId,
        options: SurfaceOptions,
        root: Element,
    ) -> Result<(), RunError> {
        let Some(fonts) = self.fonts.clone() else {
            log::error!("popup opened before the primary window loaded fonts");
            return Ok(());
        };
        let owner = self
            .primary
            .as_ref()
            .map(|chrome| Arc::clone(&chrome.window));
        let attributes = popup_attributes(&options, self.renderer, owner.as_deref());
        let window = self.create_window(event_loop, attributes)?;
        let ui = Ui::new(&self.context, root, Theme::light());
        let renderer =
            renderer::open_renderer(self.renderer, &window, &fonts).map_err(RunError::Renderer)?;
        let (mut host, mut chrome) = self.finish(window, &fonts, ui, renderer);
        redraw(&mut chrome, &mut host)?;
        self.session.insert_popup(id, host);
        self.popups.push((id, chrome));
        Ok(())
    }

    fn close_popup(&mut self, id: SurfaceId) {
        self.session.remove_popup(id);
        self.popups.retain(|(open, _)| *open != id);
    }

    fn close_primary(&mut self, event_loop: &ActiveEventLoop) {
        self.session.close_primary();
        self.primary = None;
        self.popups.clear();
        event_loop.exit();
    }

    fn slot_for(&self, id: WindowId) -> Option<WindowSlot> {
        if self
            .primary
            .as_ref()
            .is_some_and(|chrome| chrome.window.id() == id)
        {
            return Some(WindowSlot::Primary);
        }
        self.popups
            .iter()
            .position(|(_, chrome)| chrome.window.id() == id)
            .map(WindowSlot::Popup)
    }

    fn with_slot_mut<R>(
        &mut self,
        slot: WindowSlot,
        body: impl FnOnce(&mut WindowChrome, &mut WindowHost) -> R,
    ) -> Option<R> {
        match slot {
            WindowSlot::Primary => {
                let WinitHost {
                    primary, session, ..
                } = self;
                let chrome = primary.as_mut()?;
                let host = session.primary_mut()?;
                Some(body(chrome, host))
            }
            WindowSlot::Popup(index) => {
                let WinitHost {
                    popups, session, ..
                } = self;
                let (id, chrome) = popups.get_mut(index)?;
                let host = session.popup_mut(*id)?;
                Some(body(chrome, host))
            }
        }
    }

    fn redraw_window(&mut self, id: WindowId) -> Result<(), RunError> {
        let Some(slot) = self.slot_for(id) else {
            return Ok(());
        };
        let mut failure = None;
        self.with_slot_mut(slot, |chrome, host| {
            if let Err(error) = redraw(chrome, host) {
                failure = Some(error);
            }
        });
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn request_dirty_redraws(&mut self) {
        let WinitHost {
            primary,
            popups,
            session,
            ..
        } = self;
        if let (Some(chrome), Some(host)) = (primary.as_ref(), session.primary()) {
            if host.ui().needs_update() {
                chrome.window.request_redraw();
            }
        }
        for (id, chrome) in popups.iter() {
            let dirty = session
                .popup(*id)
                .is_some_and(|host| host.ui().needs_update());
            if dirty {
                chrome.window.request_redraw();
            }
        }
    }

    fn schedule(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        {
            // Motion's deadline is one 8 ms frame ahead, so it still asks the browser for a frame.
            // A later timer or caret waits until that instant instead of redrawing every frame.
            const ANIMATION_FRAME: Duration = Duration::from_millis(8);
            let now = Instant::now();
            match self.primary.as_ref().and_then(|chrome| chrome.settle_at) {
                Some(deadline) if deadline <= now + ANIMATION_FRAME => {
                    if let Some(chrome) = &self.primary {
                        chrome.window.request_redraw();
                    }
                    event_loop.set_control_flow(ControlFlow::Wait);
                }
                Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
                None => event_loop.set_control_flow(ControlFlow::Wait),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            let mut earliest = None;
            for chrome in self
                .primary
                .iter()
                .chain(self.popups.iter().map(|(_, chrome)| chrome))
            {
                match chrome.settle_at {
                    Some(deadline) if now >= deadline => chrome.window.request_redraw(),
                    Some(deadline) => {
                        earliest = Some(match earliest {
                            Some(current) if current < deadline => current,
                            _ => deadline,
                        });
                    }
                    None => {}
                }
            }
            match earliest {
                Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
                None => event_loop.set_control_flow(ControlFlow::Wait),
            }
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
        if let Err(error) = self.start(event_loop, ui) {
            self.fail(event_loop, error);
        }
        #[cfg(target_arch = "wasm32")]
        self.begin(event_loop, ui);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: Wake) {
        #[cfg(target_arch = "wasm32")]
        self.attach_renderer(event_loop);
        if self.error.is_some() {
            return;
        }
        self.apply_surface_commands(event_loop);
        if self.error.is_some() {
            return;
        }
        self.request_dirty_redraws();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        self.attach_renderer(event_loop);
        if self.error.is_some() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.open_gpu_surface();
        self.apply_surface_commands(event_loop);
        if self.error.is_some() || self.primary.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        self.schedule(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if event == WindowEvent::RedrawRequested {
            if let Err(error) = self.redraw_window(window_id) {
                self.fail(event_loop, error);
            }
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if let WindowEvent::Resized(size) = &event {
            if size.width > 0 && size.height > 0 {
                self.surface_size = (size.width, size.height);
                self.open_gpu_surface();
            }
        }
        let Some(slot) = self.slot_for(window_id) else {
            return;
        };
        if matches!(event, WindowEvent::CloseRequested) {
            match slot {
                WindowSlot::Primary => self.close_primary(event_loop),
                WindowSlot::Popup(index) => {
                    if let Some(id) = self.popups.get(index).map(|(id, _)| *id) {
                        self.close_popup(id);
                    }
                }
            }
            return;
        }
        self.with_slot_mut(slot, |chrome, host| {
            apply_window_event(chrome, host, event);
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn install_clipboard(ui: &mut Ui) {
    match arboard::Clipboard::new() {
        Ok(clipboard) => ui.set_clipboard(PlatformClipboard { clipboard }),
        Err(error) => log::warn!("clipboard unavailable, edits stay in memory: {error}"),
    }
}

#[cfg(target_arch = "wasm32")]
fn install_clipboard(ui: &mut Ui) {
    ui.set_clipboard(NoopClipboard);
}

#[cfg(not(target_arch = "wasm32"))]
struct PlatformClipboard {
    clipboard: arboard::Clipboard,
}

#[cfg(not(target_arch = "wasm32"))]
impl cappy_core::Clipboard for PlatformClipboard {
    fn read_text(&mut self) -> Option<String> {
        self.clipboard.get_text().ok()
    }

    fn write_text(&mut self, text: &str) {
        if let Err(error) = self.clipboard.set_text(text) {
            log::warn!("failed to write the clipboard: {error}");
        }
    }
}

fn apply_window_event(chrome: &mut WindowChrome, host: &mut WindowHost, event: WindowEvent) {
    let window = Arc::clone(&chrome.window);
    let events = match event {
        WindowEvent::Resized(size) => {
            host.resize((size.width, size.height));
            window.request_redraw();
            Vec::new()
        }
        WindowEvent::ScaleFactorChanged { .. } => {
            window.request_redraw();
            Vec::new()
        }
        WindowEvent::CursorMoved { position, .. } => {
            vec![translate::pointer_moved(position, window.scale_factor())]
        }
        WindowEvent::CursorLeft { .. } => vec![InputEvent::PointerLeft],
        WindowEvent::MouseInput { state, button, .. } => translate::pointer_button(button, state)
            .into_iter()
            .collect(),
        WindowEvent::MouseWheel { delta, .. } => {
            vec![translate::mouse_wheel(delta, window.scale_factor())]
        }
        WindowEvent::KeyboardInput { event, .. } if is_debug_toggle(&event) => {
            host.toggle_debug_damage();
            window.request_redraw();
            Vec::new()
        }
        WindowEvent::KeyboardInput { event, .. } => translate::keyboard(&event, chrome.modifiers),
        WindowEvent::ModifiersChanged(modifiers) => {
            let event = translate::modifiers(modifiers.state());
            if let InputEvent::Modifiers(modifiers) = event {
                chrome.modifiers = modifiers;
            }
            vec![event]
        }
        WindowEvent::Ime(event) => translate::ime(event).into_iter().collect(),
        _ => Vec::new(),
    };
    for input in events {
        let response = host.handle_event(input);
        apply_cursor(&window, response.cursor);
        apply_ime_caret(chrome, host);
        if response.redraw {
            // `about_to_wait` sleeps until `settle_at`. Leaving the caret blink there
            // holds this redraw until the caret flips.
            chrome.settle_at = Some(Instant::now());
            window.request_redraw();
        } else if response.idle {
            chrome.settle_at = response.wake_at;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn popup_attributes(
    options: &SurfaceOptions,
    renderer: RendererKind,
    owner: Option<&Window>,
) -> WindowAttributes {
    let attributes = Window::default_attributes()
        .with_title(options.title.clone())
        .with_visible(false)
        .with_inner_size(LogicalSize::new(
            options.logical_size.0,
            options.logical_size.1,
        ));
    let attributes = without_redirection_bitmap(attributes, renderer);
    owned_popup(place_near_owner(attributes, options.offset, owner), owner)
}

#[cfg(not(target_arch = "wasm32"))]
fn place_near_owner(
    attributes: WindowAttributes,
    offset: (f64, f64),
    owner: Option<&Window>,
) -> WindowAttributes {
    let Some(owner) = owner else {
        return attributes;
    };
    let Ok(position) = owner.outer_position() else {
        return attributes;
    };
    let logical = position.to_logical::<f64>(owner.scale_factor());
    attributes.with_position(LogicalPosition::new(
        logical.x + offset.0,
        logical.y + offset.1,
    ))
}

#[cfg(target_os = "windows")]
fn owned_popup(attributes: WindowAttributes, owner: Option<&Window>) -> WindowAttributes {
    let Some(owner) = owner else {
        return attributes.with_skip_taskbar(true);
    };
    let Some(hwnd) = hwnd_of(owner) else {
        log::warn!("popup has no owner handle; it opens as a normal window");
        return attributes.with_skip_taskbar(true);
    };
    attributes.with_owner_window(hwnd).with_skip_taskbar(true)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
fn owned_popup(attributes: WindowAttributes, _owner: Option<&Window>) -> WindowAttributes {
    attributes
}

#[cfg(target_os = "windows")]
fn hwnd_of(window: &Window) -> Option<HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
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

/// Moves the OS IME window when the caret rect changes. `None` leaves the last area in place.
fn apply_ime_caret(chrome: &mut WindowChrome, host: &mut WindowHost) {
    let rect = host.ui_mut().ime_caret_rect();
    if chrome.ime_caret == rect {
        return;
    }
    chrome.ime_caret = rect;
    let Some(rect) = rect else {
        return;
    };
    chrome.window.set_ime_cursor_area(
        LogicalPosition::new(f64::from(rect.x), f64::from(rect.y)),
        LogicalSize::new(
            f64::from(rect.width.max(1.0)),
            f64::from(rect.height.max(1.0)),
        ),
    );
}

fn is_debug_toggle(event: &KeyEvent) -> bool {
    event.state == ElementState::Pressed
        && !event.repeat
        && event.physical_key == PhysicalKey::Code(DEBUG_DAMAGE_KEY)
}

fn redraw(chrome: &mut WindowChrome, host: &mut WindowHost) -> Result<(), RunError> {
    let size = chrome.window.inner_size();
    if size.width == 0 || size.height == 0 {
        chrome.settle_at = None;
        if !chrome.shown {
            // No client area yet. Showing is what lets a later resize deliver a paint.
            chrome.window.set_visible(true);
            chrome.shown = true;
        }
        return Ok(());
    }
    let result = host
        .redraw((size.width, size.height), chrome.window.scale_factor())
        .map_err(RunError::Renderer)?;
    apply_cursor(&chrome.window, result.cursor);
    apply_ime_caret(chrome, host);
    chrome.settle_at = result.settle_at;
    if !chrome.shown {
        match result.present {
            PresentOutcome::Presented { .. } => {
                chrome.window.set_visible(true);
                chrome.shown = true;
                log::info!("showed the window after the first presented frame");
            }
            // A hidden window receives no WM_PAINT, and some backends report it occluded.
            // Show it and ask for another frame; the device is already open, so this is not
            // the startup wait.
            PresentOutcome::Occluded | PresentOutcome::Retry => {
                chrome.window.set_visible(true);
                chrome.shown = true;
                chrome.window.request_redraw();
            }
            PresentOutcome::Deferred => {}
        }
    }
    if result.needs_retry() {
        chrome.window.request_redraw();
    }
    Ok(())
}
