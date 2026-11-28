//! The window-manager independent half of a desktop host: owns the [`Ui`] and its renderer,
//! turns input into redraw requests and redraws into presented frames. Integrations such as
//! winit only translates its events and forwards them here.

#[cfg(debug_assertions)]
use cappy_core::ResourceCensus;
use cappy_core::time::{Duration, Instant};
use cappy_core::{CursorIcon, DebugStats, FrameInput, InputEvent, Ui, UpdateOutcome};
use cappy_text::{CosmicTextMeasurer, FontContext};

use crate::debug_overlay::{OVERLAY_IDLE, earlier, is_overlay_refresh, rolling_fps};
use crate::resize_pace::{PresentedFrame, RESIZE_INTERVAL, ResizeFrame, choose_resize_frame};
use crate::{PresentOutcome, RendererError, RendererKind, WindowRenderer};

const DEFAULT_TITLE: &str = "cappy";
const DEFAULT_LOGICAL_SIZE: (f64, f64) = (960.0, 640.0);
/// A scheduled frame inside this window applies new input. Farther out is a caret blink, and
/// waiting for it leaves the caret where it was until that blink.
const PROMPT_FRAME: Duration = Duration::from_millis(32);

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
    /// Backend to draw with; `None` uses the one compiled in.
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
    /// Set when this event applied an update and that update did not need a redraw.
    /// The host then sleeps until [`Self::wake_at`]. `None` waits for the next input.
    pub idle: bool,
    /// Next time the host should update. Meaningful when [`Self::idle`] is set.
    pub wake_at: Option<Instant>,
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
    /// when it is due within [`PROMPT_FRAME`], instead of running a second update, which
    /// freezes the window while the cursor moves over a looping animation. A later deadline
    /// is only the caret blink, so input that changes pixels does not wait for it.
    animation_due: Option<Instant>,
    /// When set, each presented frame writes a repaint line. Off by default.
    log_frames: bool,
    /// Real presents inside the trailing second. The idle panel wake is not recorded.
    presented_at: Vec<Instant>,
    /// CPU time of the last present that was not an idle panel wake.
    last_update_ms: f32,
    last_paint_ms: f32,
    /// When set, the next redraw at this instant only refreshes the panel.
    overlay_refresh_at: Option<Instant>,
    /// Previous footprint shown on the panel, so the delta is against the last sample.
    overlay_private_bytes: Option<usize>,
    /// The next present only redraws stats that were just published. It is not a measured frame.
    stats_echo: bool,
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
            presented_at: Vec::new(),
            last_update_ms: 0.0,
            last_paint_ms: 0.0,
            overlay_refresh_at: None,
            overlay_private_bytes: None,
            stats_echo: false,
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
        let (redraw, idle, wake_at) = self.settle();
        InputResponse {
            redraw,
            cursor: self.take_cursor_change(),
            idle,
            wake_at,
        }
    }

    /// Runs pending updates at the last known size.
    ///
    /// Returns whether a redraw is needed, whether the update settled with nothing to paint, and
    /// when that idle update should be woken. A motion or scroll frame due within [`PROMPT_FRAME`]
    /// applies this input on the frame already scheduled. Updating again here would restyle and
    /// rebuild the display list on every cursor event. The caret blink is hundreds of milliseconds
    /// later, so a key that moves the caret paints now. A timer armed by the event leaves
    /// `wake_at` at its deadline so the host can sleep until then.
    fn settle(&mut self) -> (bool, bool, Option<Instant>) {
        let now = Instant::now();
        if self
            .animation_due
            .is_some_and(|due| due <= now + PROMPT_FRAME)
        {
            return (false, false, None);
        }
        if !self.ui.needs_update() {
            return (false, false, None);
        }
        let Some(input) = self.last_input else {
            #[cfg(debug_assertions)]
            self.trace_memory();
            return (true, false, None);
        };
        let redraw = matches!(self.ui.update(input), UpdateOutcome::Redraw { .. });
        #[cfg(debug_assertions)]
        self.trace_memory();
        if redraw {
            return (true, false, None);
        }
        let wake_at = self.ui.frame_deadline(now);
        self.animation_due = wake_at;
        (false, true, wake_at)
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
            RESIZE_INTERVAL,
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
        let now = Instant::now();
        let echo = self.stats_echo;
        self.stats_echo = false;
        let refresh_due = self.overlay_refresh_at.is_some_and(|at| at <= now);
        let ui_due = self.ui.frame_deadline(now).is_some_and(|due| due <= now);
        let ui_dirty = self.ui.needs_update();
        let refreshing = is_overlay_refresh(refresh_due, ui_due, ui_dirty);
        // Publishing stats wakes one frame so the binding can draw them. That frame is not a
        // sample, or the panel would keep the window awake.
        let displaying = echo && !ui_due && !refreshing;
        if refreshing && self.ui.wants_debug_stats() {
            let fps = rolling_fps(&mut self.presented_at, now, false);
            let stats = self.measure_stats(fps);
            self.stats_echo = self.ui.publish_debug(stats);
        }
        let input = FrameInput::new(physical_size.0, physical_size.1, scale);
        self.last_input = Some(input);
        let first_frame = self.frames == 0;
        if first_frame {
            memory_mark("frame 1 before update");
        }
        let update_started = Instant::now();
        self.ui.update(input);
        if first_frame {
            memory_mark("frame 1 after update");
        }
        let frame = self
            .ui
            .prepare_frame()
            .expect("a frame is always available after an update");
        let update_ms = elapsed_ms(update_started);
        if first_frame {
            memory_mark("frame 1 after prepare");
        }
        let wanted = self.ui.wants_debug_stats();
        self.renderer
            .set_collect_stats(wanted && !displaying && !refreshing);
        let background = self.ui.theme().tokens().colors.background;
        let size = (physical_size.0, physical_size.1);
        if self.presented.map(|frame| frame.size) != Some(size) {
            self.renderer.resize(size);
            log::debug!("resized to {:?}", size);
            if first_frame {
                memory_mark("frame 1 after resize");
            }
        }
        let paint_started = Instant::now();
        let present = self.renderer.present(&frame, background)?;
        let paint_ms = elapsed_ms(paint_started);
        if first_frame {
            memory_mark("frame 1 after present");
        }
        let presented_frame = matches!(present, PresentOutcome::Presented { .. });
        if presented_frame {
            self.presented = Some(PresentedFrame {
                size: (physical_size.0, physical_size.1),
                scale_factor: scale,
                at: Instant::now(),
            });
        }
        if wanted && presented_frame && !displaying && !refreshing {
            let fps = rolling_fps(&mut self.presented_at, now, true);
            self.last_update_ms = update_ms;
            self.last_paint_ms = paint_ms;
            let stats = self.measure_stats(fps);
            self.stats_echo = self.ui.publish_debug(stats);
        }
        if !wanted {
            self.presented_at.clear();
            self.overlay_refresh_at = None;
            self.overlay_private_bytes = None;
        }
        self.frames += 1;
        if self.log_frames
            && let PresentOutcome::Presented { repainted } = present
        {
            log::info!(
                "frame {}: repainted {repainted} region(s), damage {:?}",
                self.frames, frame.damage
            );
        }
        #[cfg(debug_assertions)]
        self.trace_memory();
        let settle_at = self.stats_deadline(wanted, presented_frame, refreshing, displaying);
        self.animation_due = settle_at;
        Ok(RedrawResult {
            present,
            cursor: self.take_cursor_change(),
            settle_at,
        })
    }

    /// Census, footprint and the last counted paint. Called only while a binding is subscribed.
    fn measure_stats(&mut self, fps: u32) -> DebugStats {
        let resources = self.ui.resources();
        let (private_bytes, private_delta_kib) = self.sample_overlay_memory();
        let paint = self.renderer.paint_stats();
        DebugStats {
            fps,
            update_ms: self.last_update_ms,
            paint_ms: self.last_paint_ms,
            private_bytes,
            private_delta_kib,
            live_nodes: resources.live_nodes,
            node_capacity: resources.node_capacity,
            primitives: resources.display_list,
            batches: paint.batches,
            draw_calls: paint.draw_calls,
            damage_regions: paint.regions,
            damage_percent: paint.coverage_percent,
            live_signals: resources.live_signals,
            bindings: resources.bindings,
            layout_nodes: resources.layout_nodes,
            layout_retained: resources.layout_retained,
        }
    }

    fn sample_overlay_memory(&mut self) -> (Option<usize>, i64) {
        let Some((private_bytes, _)) = process_memory() else {
            return (None, 0);
        };
        let delta_kib = self.overlay_private_bytes.map_or(0, |previous| {
            let delta = private_bytes as isize - previous as isize;
            i64::try_from(delta / 1024).unwrap_or(0)
        });
        self.overlay_private_bytes = Some(private_bytes);
        (Some(private_bytes), delta_kib)
    }

    /// UI deadline, or one second after a real present while stats are subscribed and nothing
    /// else is scheduled sooner. The idle wake runs once. The echo frame that draws a publish
    /// keeps that deadline instead of measuring itself.
    fn stats_deadline(
        &mut self,
        wanted: bool,
        presented_frame: bool,
        refreshing: bool,
        displaying: bool,
    ) -> Option<Instant> {
        let now = Instant::now();
        let ui_deadline = self.next_frame_at(now);
        if !wanted || !presented_frame {
            self.overlay_refresh_at = None;
            return ui_deadline;
        }
        if refreshing {
            self.overlay_refresh_at = None;
            return ui_deadline;
        }
        if displaying {
            return earlier(ui_deadline, self.overlay_refresh_at);
        }
        self.overlay_refresh_at = Some(now + OVERLAY_IDLE);
        earlier(ui_deadline, self.overlay_refresh_at)
    }

    /// When the next animation frame is due. Vsync absorbs a short step, so the core's deadline
    /// is used as it is. A later deadline, such as a timer or the caret, waits until that instant.
    fn next_frame_at(&self, now: Instant) -> Option<Instant> {
        self.ui.frame_deadline(now)
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

fn elapsed_ms(since: Instant) -> f32 {
    since.elapsed().as_secs_f32() * 1_000.0
}

/// Footprint and resident bytes of this process.
///
/// The footprint is what the panel calls memory: private bytes on Windows, `phys_footprint` on
/// macOS (the number Activity Monitor shows). The resident value is the working set. Wasm and
/// other targets return `None`; the panel prints `n/a` and still shows the arena counts.
fn process_memory() -> Option<(usize, usize)> {
    #[cfg(windows)]
    {
        process_memory_windows::read()
    }
    #[cfg(target_os = "macos")]
    {
        process_memory_macos::read()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

/// `GetProcessMemoryInfo` has no safe wrapper in this crate's dependencies.
#[cfg(windows)]
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

/// `task_info` has no safe wrapper in this crate's dependencies.
///
/// The struct is `task_vm_info` through `phys_footprint` (rev1). The count passed to the kernel
/// is that prefix, so a newer kernel still fills the fields this panel reads.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod process_memory_macos {
    pub(super) fn read() -> Option<(usize, usize)> {
        const TASK_VM_INFO: u32 = 22;

        #[repr(C)]
        struct TaskVmInfo {
            virtual_size: u64,
            region_count: i32,
            page_size: i32,
            resident_size: u64,
            resident_size_peak: u64,
            device: u64,
            device_peak: u64,
            internal: u64,
            internal_peak: u64,
            external: u64,
            external_peak: u64,
            reusable: u64,
            reusable_peak: u64,
            purgeable_volatile_pmap: u64,
            purgeable_volatile_resident: u64,
            purgeable_volatile_virtual: u64,
            compressed: u64,
            compressed_peak: u64,
            compressed_lifetime: u64,
            phys_footprint: u64,
        }

        unsafe extern "C" {
            static mach_task_self_: u32;
            fn task_info(
                target_task: u32,
                flavor: u32,
                task_info_out: *mut i32,
                task_info_count: *mut u32,
            ) -> i32;
        }

        unsafe {
            let mut info = TaskVmInfo {
                virtual_size: 0,
                region_count: 0,
                page_size: 0,
                resident_size: 0,
                resident_size_peak: 0,
                device: 0,
                device_peak: 0,
                internal: 0,
                internal_peak: 0,
                external: 0,
                external_peak: 0,
                reusable: 0,
                reusable_peak: 0,
                purgeable_volatile_pmap: 0,
                purgeable_volatile_resident: 0,
                purgeable_volatile_virtual: 0,
                compressed: 0,
                compressed_peak: 0,
                compressed_lifetime: 0,
                phys_footprint: 0,
            };
            let mut count = u32::try_from(size_of::<TaskVmInfo>() / size_of::<u32>()).ok()?;
            let status = task_info(
                mach_task_self_,
                TASK_VM_INFO,
                (&mut info as *mut TaskVmInfo).cast(),
                &mut count,
            );
            (status == 0 && info.phys_footprint > 0).then_some((
                usize::try_from(info.phys_footprint).ok()?,
                usize::try_from(info.resident_size).ok()?,
            ))
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::process_memory;

    #[test]
    fn macos_reports_a_nonzero_footprint() {
        let (footprint, resident) = process_memory().expect("task_info");
        assert!(footprint > 0);
        assert!(resident > 0);
    }
}
