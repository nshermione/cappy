//! The canvas tab. One immediate stage redraws every frame; the rest of the window does not.
//! Leaving the tab hides the stage, and a hidden canvas does not ask for another frame.

use std::cell::Cell;
use std::rc::Rc;

use cappy::Class as ThemeClass;
use cappy::Rect;
use cappy::prelude::*;
use cappy::time::Instant;

use super::style::Class;

const ORB: Color = Color::rgb(14, 165, 233);
const MOON: Color = Color::rgb(251, 191, 36);
const SCAN: Color = Color::rgba(56, 189, 248, 170);
const GROUND: Color = Color::rgba(148, 163, 184, 120);

#[component]
pub struct CanvasTab {}

impl Component for CanvasTab {
    fn render(&self) -> Element {
        element! {
            div(class = [ThemeClass::Card, Class::Grow]) {
                text("Canvas", class = ThemeClass::Subtitle)
                text(
                    "This stage redraws every frame. The rest of the window stays put. Another tab stops it.",
                    class = ThemeClass::Muted,
                ),
                { stage() }
            }
        }
    }
}

fn stage() -> Element {
    let origin: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    canvas(move |canvas, frame| {
        let start = match origin.get() {
            Some(start) => start,
            None => {
                origin.set(Some(frame.now));
                frame.now
            }
        };
        let elapsed = frame.now.saturating_duration_since(start).as_secs_f32();
        paint_stage(canvas, frame, elapsed);
        CanvasTick::Continue
    })
    .key("canvas-stage")
    .class(Class::CanvasStage)
    .style(StyleRefinement::default().background(Color::hex("#0f172a")))
}

fn paint_stage(canvas: &mut Canvas, frame: &CanvasFrame, elapsed: f32) {
    let width = frame.size.width;
    let height = frame.size.height;
    if width < 8.0 || height < 8.0 {
        return;
    }
    canvas.fill(
        Rect::new(20.0, height - 28.0, (width - 40.0).max(0.0), 2.0),
        GROUND,
    );
    let span = (width - 64.0).max(1.0);
    let scan = (elapsed * 140.0) % span;
    canvas.fill(
        Rect::new(32.0 + scan, 20.0, 3.0, (height - 56.0).max(0.0)),
        SCAN,
    );
    let reach_x = (width * 0.28).min(160.0);
    let reach_y = (height * 0.28).min(70.0);
    let angle = elapsed * 1.6;
    let origin_x = width * 0.5;
    let origin_y = height * 0.46;
    blot(
        canvas,
        origin_x + angle.cos() * reach_x,
        origin_y + angle.sin() * reach_y,
        28.0,
        ORB,
    );
    let angle = angle + std::f32::consts::PI;
    blot(
        canvas,
        origin_x + angle.cos() * reach_x,
        origin_y + angle.sin() * reach_y,
        16.0,
        MOON,
    );
}

fn blot(canvas: &mut Canvas, x: f32, y: f32, size: f32, color: Color) {
    canvas.fill_rounded(
        Rect::new(x - size * 0.5, y - size * 0.5, size, size),
        size * 0.5,
        color,
    );
}
