//! Immediate drawing for one leaf. The callback runs again each frame until it rests.
//! Coordinates are local to the element's content box, origin at the top left.

use cappy_layout::{Rect, Size};
use cappy_style::Color;

use crate::time::{Duration, Instant};

/// What a [`Canvas`] callback does after drawing this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasTick {
    /// Draw again on the next frame.
    Continue,
    /// Keep the picture from this frame and stop requesting frames.
    Rest,
}

/// Clock and box handed to a canvas callback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasFrame {
    pub now: Instant,
    /// Time since the previous canvas frame. The first frame, a frame after the canvas was
    /// hidden, and a gap longer than a tenth of a second are zero, so a resume does not jump.
    pub dt: Duration,
    /// Content box, in logical pixels. Padding is already removed.
    pub size: Size,
}

impl CanvasFrame {
    /// [`Self::dt`] in seconds.
    pub fn dt_secs(self) -> f32 {
        self.dt.as_secs_f32()
    }
}

/// Records fills for one canvas frame. The next frame starts empty.
pub struct Canvas {
    commands: Vec<CanvasCommand>,
}

impl Canvas {
    pub(crate) fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub(crate) fn into_commands(self) -> Vec<CanvasCommand> {
        self.commands
    }

    /// Fills `rect` in content-box coordinates.
    pub fn fill(&mut self, rect: Rect, color: Color) {
        self.push(rect, color, 0.0);
    }

    /// Fills `rect` with the same radius on every corner. `radius` is in logical pixels.
    pub fn fill_rounded(&mut self, rect: Rect, radius: f32, color: Color) {
        self.push(rect, color, radius.max(0.0));
    }

    fn push(&mut self, rect: Rect, color: Color, radius: f32) {
        if rect.is_empty() || color.is_transparent() {
            return;
        }
        self.commands.push(CanvasCommand {
            rect,
            color,
            radius,
        });
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasCommand {
    pub(crate) rect: Rect,
    pub(crate) color: Color,
    pub(crate) radius: f32,
}

pub(crate) type CanvasDraw = Box<dyn Fn(&mut Canvas, &CanvasFrame) -> CanvasTick>;

pub(crate) struct CanvasNode {
    pub(crate) draw: Option<CanvasDraw>,
    pub(crate) commands: Vec<CanvasCommand>,
    pub(crate) playing: bool,
    pub(crate) clock: Option<Instant>,
}
