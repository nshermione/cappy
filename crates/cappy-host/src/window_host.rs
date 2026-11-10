//! The window-manager independent half of a desktop host: owns the [`Ui`] and its renderer,
//! turns input into redraw requests and redraws into presented frames. Integrations such as
//! winit only translates its events and forwards them here.

#[cfg(debug_assertions)]
use cappy_core::ResourceCensus;
use cappy_core::time::Instant;
use cappy_core::{CursorIcon, FrameInput, InputEvent, Ui, UpdateOutcome};
use cappy_text::{CosmicTextMeasurer, FontContext};

use crate::resize_pace::{PresentedFrame, ResizeFrame, choose_resize_frame, resize_interval};
use crate::{PresentOutcome, RendererError, RendererKind, WindowRenderer};

const DEFAULT_TITLE: &str = "cappy";
const DEFAULT_LOGICAL_SIZE: (f64, f64) = (960.0, 640.0);

/// Window settings shared by the desktop integrations.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowOptions {
    pub title: String,
    pub logical_size: (f64, f64),
    /// Outline repainted regions; toggled at runtime with F12.
    pub debug_damage: bool,
    /// Log each presented frame's repaint regions. Off unless a host opts in.
    pub log_frames: bool,
    /// Sample this process's memory and log it. Off unless a host opts in.
    /// Debug builds only; release builds record nothing either way.
    pub log_memory: bool,
    /// Backend to draw with; `None` picks the first one compiled in (GPU before CPU).
    /// The `CAPPY_RENDERER` environment variable overrides this.
    pub renderer: Option<RendererKind>,
    /// Which system faces to keep, and any extra font files to load.
    pub fonts: cappy_text::FontOptions,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: DEFAULT_TITLE.to_owned(),
            logical_size: DEFAULT_LOGICAL_SIZE,
            debug_damage: false,
            log_frames: false,
            log_memory: false,
            renderer: None,
            fonts: cappy_text::FontOptions::new(),
        }
    }
}

impl WindowOptions {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_size(mut self, width: f64, height: f64) -> Self {
        self.logical_size = (width, height);
        self
    }

    #[must_use]
    pub fn with_debug_damage(mut self, enabled: bool) -> Self {
        self.debug_damage = enabled;
        self
    }

    /// Logs one line per presented frame: how many regions were repainted and the damage.
    #[must_use]
    pub fn with_log_frames(mut self, enabled: bool) -> Self {
        self.log_frames = enabled;
        self
    }

    /// Samples process memory at startup stages and whenever the census changes.
    /// Has no effect in release builds.
    #[must_use]
    pub fn with_log_memory(mut self, enabled: bool) -> Self {
        self.log_memory = enabled;
        self
    }

    #[must_use]
    pub fn with_renderer(mut self, renderer: RendererKind) -> Self {
        self.renderer = Some(renderer);
        self
    }

    #[must_use]
    pub fn with_fonts(mut self, fonts: cappy_text::FontOptions) -> Self {
        self.fonts = fonts;
        self
    }
}

/// What the host window should do after an input event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputResponse {
    /// Set when the event produced damage, so a redraw must be requested.
    pub redraw: bool,
    /// Set when the cursor icon must change.
    pub cursor: Option<CursorIcon>,
}

/// What the host window should do after a redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedrawResult {
    pub present: PresentOutcome,
    /// Set when the cursor icon must change.
    pub cursor: Option<CursorIcon>,
    /// When set, redraw at this instant. A drag in progress presents on this cadence, and a
    /// released drag presents the resting size here.
    pub settle_at: Option<Instant>,
}

impl RedrawResult {
    /// Whether the window should schedule another redraw right away.
    pub fn needs_retry(&self) -> bool {
        self.present == PresentOutcome::Retry
    }
}

pub struct WindowHost {
    ui: Ui,
    renderer: Box<dyn WindowRenderer>,
    cursor: CursorIcon,
    /// Size of the last redraw, used to settle input without waiting for a frame.
    last_input: Option<FrameInput>,
    /// Last size passed to [`WindowHost::resize`], and when it changed.
    last_resize_size: Option<(u32, u32)>,
    last_resize: Option<Instant>,
    /// Last size actually submitted to the GPU.
    presented: Option<PresentedFrame>,
    frames: u64,
    /// When set, a motion or scroll frame is already scheduled. Pointer events wait for it
    /// instead of running a second update, which freezes the window while the cursor moves
    /// over a looping animation.
    animation_due: Option<Instant>,
    /// When set, each presented frame writes a repaint line. Off by default.
    log_frames: bool,
    /// Previous resource snapshot, so the memory trace logs only when something changed.
    #[cfg(debug_assertions)]
    traced_resources: Option<ResourceCensus>,
    /// Previous private bytes of this process, from the OS.
    #[cfg(debug_assertions)]
    traced_private_bytes: Option<usize>,
}

impl WindowHost {
    /// Attaches `ui` to `renderer`, measuring text with `fonts` (the fonts the renderer draws).
    pub fn new(renderer: Box<dyn WindowRenderer>, fonts: &FontContext, mut ui: Ui) -> Self {
        ui.set_text_measurer(CosmicTextMeasurer::new(fonts));
        log::info!("cappy window host using the {} renderer", renderer.kind());
        Self {
            ui,
            renderer,
            cursor: CursorIcon::Default,
            last_input: None,
            last_resize_size: None,
            last_resize: None,
            presented: None,
            frames: 0,
            animation_due: None,
            log_frames: false,
            #[cfg(debug_assertions)]
            traced_resources: None,
            #[cfg(debug_assertions)]
            traced_private_bytes: None,
        }
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    pub fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
    }

    pub fn renderer_kind(&self) -> RendererKind {
        self.renderer.kind()
    }

    /// Lets signal writes from outside the event loop wake the window.
    pub fn set_waker(&mut self, waker: impl Fn() + 'static) {
        self.ui.set_waker(waker);
    }

    pub fn set_debug_damage(&mut self, enabled: bool) {
        self.renderer.set_debug_damage(enabled);
    }

    pub fn set_log_frames(&mut self, enabled: bool) {
        self.log_frames = enabled;
    }

    pub fn toggle_debug_damage(&mut self) {
        let enabled = self.renderer.debug_damage();
        self.renderer.set_debug_damage(!enabled);
    }

    /// Feeds input and settles the UI right away, so input that changes nothing visible (e.g.
    /// hovering plain text) does not cost a frame.
    pub fn handle_event(&mut self, event: InputEvent) -> InputResponse {
        self.ui.handle_event(event);
        InputResponse {
            redraw: self.settle(),
            cursor: self.take_cursor_change(),
        }
    }

    /// Runs pending updates at the last known size. Returns whether a redraw is needed.
    ///
    /// A frame already scheduled for motion or scroll applies this input. Updating again here
    /// would restyle and rebuild the display list on every cursor event.
    fn settle(&mut self) -> bool {
        if self.animation_due.is_some() {
            return false;
        }
        if !self.ui.needs_update() {
            return false;
        }
        let redraw = match self.last_input {
            Some(input) => matches!(self.ui.update(input), UpdateOutcome::Redraw { .. }),
            None => true,
        };
        #[cfg(debug_assertions)]
        self.trace_memory();
        redraw
    }

    /// Records a window-size change for the present pace. The surface is resized in
    /// [`WindowHost::redraw`] when that size is actually shown.
    pub fn resize(&mut self, physical_size: (u32, u32)) {
        if physical_size.0 == 0
            || physical_size.1 == 0
            || self.last_resize_size == Some(physical_size)
        {
            return;
        }
        self.last_resize_size = Some(physical_size);
        self.last_resize = Some(Instant::now());
    }

    /// Updates the UI for the current window size and presents a frame. When nothing changed the
    /// frame carries no damage, so this costs only a composite (e.g. after the OS exposes the
    /// window).
    ///
    /// While the window size is still changing, presents are at least one resize interval apart.
    /// [`RedrawResult::settle_at`] is when the next one is due, including the frame for the
    /// resting size.
    pub fn redraw(
        &mut self,
        physical_size: (u32, u32),
        scale_factor: f64,
    ) -> Result<RedrawResult, RendererError> {
        let size = (physical_size.0, physical_size.1);
        let scale = scale_factor as f32;
        let now = Instant::now();
        if let ResizeFrame::Defer { after } = choose_resize_frame(
            self.presented,
            size,
            scale,
            self.last_resize,
            now,
            resize_interval(self.renderer.kind()),
        ) {
            return Ok(RedrawResult {
                present: PresentOutcome::Deferred,
                cursor: None,
                settle_at: Some(now + after),
            });
        }
        self.paint_and_present(physical_size, scale)
    }

    fn paint_and_present(
        &mut self,
        physical_size: (u32, u32),
        scale: f32,
    ) -> Result<RedrawResult, RendererError> {
        let input = FrameInput::new(physical_size.0, physical_size.1, scale);
        self.last_input = Some(input);
        let first_frame = self.frames == 0;
        if first_frame {
            memory_mark("frame 1 before update");
        }
        self.ui.update(input);
        if first_frame {
            memory_mark("frame 1 after update");
        }
        let frame = self
            .ui
            .prepare_frame()
            .expect("a frame is always available after an update");
        if first_frame {
            memory_mark("frame 1 after prepare");
        }
        let background = self.ui.theme().tokens().colors.background;
        let size = (physical_size.0, physical_size.1);
        if self.presented.map(|frame| frame.size) != Some(size) {
            self.renderer.resize(size);
            log::debug!("resized to {:?}", size);
            if first_frame {
                memory_mark("frame 1 after resize");
            }
        }
        let present = self.renderer.present(&frame, background)?;
        if first_frame {
            memory_mark("frame 1 after present");
        }
        if matches!(present, PresentOutcome::Presented { .. }) {
            self.presented = Some(PresentedFrame {
                size: (physical_size.0, physical_size.1),
                scale_factor: scale,
                at: Instant::now(),
            });
        }
        self.frames += 1;
        if self.log_frames {
            if let PresentOutcome::Presented { repainted } = present {
                log::info!(
                    "frame {}: repainted {repainted} region(s), damage {:?}",
                    self.frames,
                    frame.damage
                );
            }
        }
        #[cfg(debug_assertions)]
        self.trace_memory();
        let settle_at = self.ui.frame_deadline(Instant::now());
        self.animation_due = settle_at;
        Ok(RedrawResult {
            present,
            cursor: self.take_cursor_change(),
            settle_at,
        })
    }

    /// Logs cappy's arena counts next to the process's private bytes.
    ///
    /// Compiled only into debug builds. Each call walks the signal and subscriber lists and asks
    /// the OS for this process's memory, and a change writes a log line.
    ///
    /// A cappy counter that returns to an earlier value released that arena. Private bytes that
    /// stay up, or step up while the counters repeat, are the process heap and renderer caches
    /// this census does not hold.
    #[cfg(debug_assertions)]
    fn trace_memory(&mut self) {
        if !log_memory_enabled() {
            return;
        }
        let resources = self.ui.resources();
        let memory = process_memory();
        let resources_changed = self.traced_resources.as_ref() != Some(&resources);
        let private_delta = memory.and_then(|(private_bytes, _)| {
            self.traced_private_bytes
                .map(|previous| private_bytes as isize - previous as isize)
        });
        let memory_changed = private_delta.is_none_or(|delta| delta.unsigned_abs() >= 32 * 1024);
        if !resources_changed && !memory_changed {
            return;
        }
        if let Some((private_bytes, working_set)) = memory {
            let delta_kib = private_delta.unwrap_or(0) / 1024;
            log::info!(
                "memory private {} KiB ({delta_kib:+} KiB) working {} KiB | cappy nodes {}/{} layout {}/{} text {} signals {} bindings {}/{} listeners {}/{} singletons {}",
                private_bytes / 1024,
                working_set / 1024,
                resources.nodes,
                resources.node_capacity,
                resources.layout_nodes,
                resources.layout_retained,
                resources.text_measurements,
                resources.live_signals,
                resources.bindings,
                resources.binding_capacity,
                resources.live_listeners,
                resources.listener_capacity,
                resources.singletons,
            );
            self.traced_private_bytes = Some(private_bytes);
        } else {
            log::info!(
                "memory | cappy nodes {}/{} layout {}/{} text {} signals {} bindings {}/{}",
                resources.nodes,
                resources.node_capacity,
                resources.layout_nodes,
                resources.layout_retained,
                resources.text_measurements,
                resources.live_signals,
                resources.bindings,
                resources.binding_capacity,
            );
        }
        self.traced_resources = Some(resources);
    }

    fn take_cursor_change(&mut self) -> Option<CursorIcon> {
        let cursor = self.ui.cursor();
        (cursor != self.cursor).then(|| {
            self.cursor = cursor;
            cursor
        })
    }
}

/// Turns process-memory sampling on or off.
///
/// [`memory_mark`] and the host census both read this. It starts off. A window sets it from
/// [`WindowOptions::log_memory`] before the first sample. Release builds ignore it.
pub fn set_log_memory(enabled: bool) {
    #[cfg(debug_assertions)]
    {
        use std::sync::atomic::Ordering;
        LOG_MEMORY.store(enabled, Ordering::Relaxed);
    }
    #[cfg(not(debug_assertions))]
    let _ = enabled;
}

#[cfg(debug_assertions)]
fn log_memory_enabled() -> bool {
    use std::sync::atomic::Ordering;
    LOG_MEMORY.load(Ordering::Relaxed)
}

#[cfg(debug_assertions)]
static LOG_MEMORY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Logs this process's private bytes at `stage`, and how much they moved since the previous mark.
///
/// Debug builds only, and only while [`set_log_memory`] is on. The delta is in KiB, same as the
/// totals. Release builds record nothing and do not ask the OS.
#[cfg(not(debug_assertions))]
pub fn memory_mark(_stage: &str) {}

#[cfg(debug_assertions)]
pub fn memory_mark(stage: &str) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    if !log_memory_enabled() {
        return;
    }
    static PREVIOUS: AtomicUsize = AtomicUsize::new(0);
    let Some((private_bytes, working_set)) = process_memory() else {
        return;
    };
    let previous = PREVIOUS.swap(private_bytes, Ordering::Relaxed);
    let delta_kib = if previous == 0 {
        0
    } else {
        (private_bytes as isize - previous as isize) / 1024
    };
    log::info!(
        "memory stage {stage}: private {} KiB ({delta_kib:+} KiB) working {} KiB",
        private_bytes / 1024,
        working_set / 1024,
    );
}

/// Private bytes and working set of this process. Private bytes are what the process has asked
/// the OS to keep; the working set is the part currently in RAM.
#[cfg(debug_assertions)]
fn process_memory() -> Option<(usize, usize)> {
    #[cfg(windows)]
    {
        process_memory_windows::read()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// `GetProcessMemoryInfo` has no safe wrapper in this crate's dependencies.
#[cfg(all(debug_assertions, windows))]
#[allow(unsafe_code)]
mod process_memory_windows {
    pub(super) fn read() -> Option<(usize, usize)> {
        #[repr(C)]
        struct Counters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
            private_usage: usize,
        }

        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut core::ffi::c_void;
            fn K32GetProcessMemoryInfo(
                process: *mut core::ffi::c_void,
                counters: *mut Counters,
                cb: u32,
            ) -> i32;
        }

        unsafe {
            let mut counters = Counters {
                cb: size_of::<Counters>() as u32,
                page_fault_count: 0,
                peak_working_set_size: 0,
                working_set_size: 0,
                quota_peak_paged_pool_usage: 0,
                quota_paged_pool_usage: 0,
                quota_peak_non_paged_pool_usage: 0,
                quota_non_paged_pool_usage: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
                private_usage: 0,
            };
            let read = K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb);
            (read != 0).then_some((counters.private_usage, counters.working_set_size))
        }
    }
}
