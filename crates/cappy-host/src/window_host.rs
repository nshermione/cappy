//! The window-manager independent half of a desktop host: owns the [`Ui`] and its renderer,
//! turns input into redraw requests and redraws into presented frames. Integrations such as
//! winit and tao only translate their events and forward them here.

use cappy_core::{CursorIcon, FrameInput, InputEvent, Ui, UpdateOutcome};
use cappy_text::{CosmicTextMeasurer, FontContext};

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
    /// Backend to draw with; `None` picks the first one compiled in (GPU before CPU).
    /// The `CAPPY_RENDERER` environment variable overrides this.
    pub renderer: Option<RendererKind>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: DEFAULT_TITLE.to_owned(),
            logical_size: DEFAULT_LOGICAL_SIZE,
            debug_damage: false,
            renderer: None,
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

    #[must_use]
    pub fn with_renderer(mut self, renderer: RendererKind) -> Self {
        self.renderer = Some(renderer);
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
    frames: u64,
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
            frames: 0,
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
    fn settle(&mut self) -> bool {
        if !self.ui.needs_update() {
            return false;
        }
        match self.last_input {
            Some(input) => matches!(self.ui.update(input), UpdateOutcome::Redraw { .. }),
            None => true,
        }
    }

    pub fn resize(&mut self, physical_size: (u32, u32)) {
        self.renderer.resize(physical_size);
    }

    /// Updates the UI for the current window size and presents a frame. When nothing changed the
    /// frame carries no damage, so this costs only a composite (e.g. after the OS exposes the
    /// window).
    pub fn redraw(
        &mut self,
        physical_size: (u32, u32),
        scale_factor: f64,
    ) -> Result<RedrawResult, RendererError> {
        let input = FrameInput::new(physical_size.0, physical_size.1, scale_factor as f32);
        self.last_input = Some(input);
        self.ui.update(input);
        let frame = self
            .ui
            .prepare_frame()
            .expect("a frame is always available after an update");
        let background = self.ui.theme().tokens().colors.background;
        let present = self.renderer.present(&frame, background)?;
        self.frames += 1;
        if let PresentOutcome::Presented { repainted } = present {
            log::debug!(
                "frame {}: repainted {repainted} region(s), damage {:?}",
                self.frames,
                frame.damage
            );
        }
        Ok(RedrawResult {
            present,
            cursor: self.take_cursor_change(),
        })
    }

    fn take_cursor_change(&mut self) -> Option<CursorIcon> {
        let cursor = self.ui.cursor();
        (cursor != self.cursor).then(|| {
            self.cursor = cursor;
            cursor
        })
    }
}
