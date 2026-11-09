//! Scroll offset easing and scrollbar geometry.
//!
//! A wheel event moves the target; the offset chases it and stops there. Dragging the scrollbar
//! writes the offset directly so the thumb stays under the pointer.

use std::time::{Duration, Instant};

use cappy_layout::{Point, Rect};
use cappy_style::Color;

/// Time constant of the exponential ease, in seconds.
pub(crate) const SCROLL_TAU: f32 = 0.05;
/// Offsets closer than this snap to the target and the animation ends.
pub(crate) const SCROLL_SNAP: f32 = 0.5;
/// Delay before the next ease frame. The host waits; it does not poll.
pub(crate) const SCROLL_FRAME: Duration = Duration::from_millis(8);
/// Longer gaps are time the UI spent idle, not a frame of this ease.
const SCROLL_IDLE_GAP: f32 = 0.1;

const THUMB_THICKNESS: f32 = 4.0;
const THUMB_INSET: f32 = 2.0;
const THUMB_MIN: f32 = 16.0;
/// Grab strip along a scrolling edge. The painted thumb is thinner than this.
const SCROLLBAR_HIT: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollAxis {
    Horizontal,
    Vertical,
}

pub(crate) const THUMB_COLOR: Color = Color::rgba(100, 116, 139, 160);

/// Seconds of ease to apply. A pause longer than [`SCROLL_IDLE_GAP`] contributes nothing, so the
/// first event after idle does not finish the motion in one step.
pub(crate) fn scroll_dt(previous: Option<Instant>, now: Instant) -> f32 {
    let Some(previous) = previous else {
        return 0.0;
    };
    let dt = now.saturating_duration_since(previous).as_secs_f32();
    if dt > SCROLL_IDLE_GAP { 0.0 } else { dt }
}

/// Moves `current` toward `target`. The result stays on the segment between them.
pub(crate) fn ease_toward(current: f32, target: f32, dt: f32) -> f32 {
    let delta = target - current;
    if delta.abs() <= SCROLL_SNAP {
        return target;
    }
    if dt <= 0.0 {
        return current;
    }
    let factor = (1.0 - (-dt / SCROLL_TAU).exp()).clamp(0.0, 1.0);
    let next = current + delta * factor;
    if (target - next).abs() <= SCROLL_SNAP {
        target
    } else {
        next
    }
}

pub(crate) fn ease_point(current: Point, target: Point, dt: f32) -> Point {
    Point::new(
        ease_toward(current.x, target.x, dt),
        ease_toward(current.y, target.y, dt),
    )
}

pub(crate) fn scrolling(current: Point, target: Point) -> bool {
    (current.x - target.x).abs() > SCROLL_SNAP || (current.y - target.y).abs() > SCROLL_SNAP
}

/// The box children are clipped to: the border box inset by the painted border.
pub(crate) fn scrollport(rect: Rect, border: f32) -> Rect {
    let border = border.max(0.0);
    Rect::new(
        rect.x + border,
        rect.y + border,
        (rect.width - border * 2.0).max(0.0),
        (rect.height - border * 2.0).max(0.0),
    )
}

/// Clip rect for a scrollport. An axis that does not clip stays open.
pub(crate) fn axis_clip(port: Rect, clip_x: bool, clip_y: bool) -> Rect {
    const OPEN: f32 = 1.0e6;
    let (x, width) = if clip_x {
        (port.x, port.width)
    } else {
        (port.x - OPEN, port.width + OPEN * 2.0)
    };
    let (y, height) = if clip_y {
        (port.y, port.height)
    } else {
        (port.y - OPEN, port.height + OPEN * 2.0)
    };
    Rect::new(x, y, width, height)
}

pub(crate) fn nest_clip(outer: Option<Rect>, inner: Rect) -> Option<Rect> {
    match outer {
        Some(outer) => outer.intersection(&inner),
        None => Some(inner),
    }
}

/// Overlay thumb for one axis. `offset` and `max_offset` are in logical pixels.
pub(crate) fn vertical_thumb(port: Rect, offset: f32, max_offset: f32) -> Option<Rect> {
    thumb_along(port, offset, max_offset, true)
}

pub(crate) fn horizontal_thumb(port: Rect, offset: f32, max_offset: f32) -> Option<Rect> {
    thumb_along(port, offset, max_offset, false)
}

/// Strip along `port`'s far edge that receives presses for `axis`.
pub(crate) fn scrollbar_track(port: Rect, axis: ScrollAxis) -> Rect {
    match axis {
        ScrollAxis::Vertical => {
            let width = SCROLLBAR_HIT.min(port.width);
            Rect::new(port.right() - width, port.y, width, port.height)
        }
        ScrollAxis::Horizontal => {
            let height = SCROLLBAR_HIT.min(port.height);
            Rect::new(port.x, port.bottom() - height, port.width, height)
        }
    }
}

/// Scroll offset that puts the thumb's grab point on `pointer`.
///
/// `grab` is the distance from the thumb's leading edge to the pointer, in logical pixels.
pub(crate) fn offset_for_grab(
    port_origin: f32,
    port_span: f32,
    pointer: f32,
    grab: f32,
    max_offset: f32,
) -> f32 {
    if max_offset <= 0.0 {
        return 0.0;
    }
    let track = port_span - THUMB_INSET * 2.0;
    if track <= 0.0 {
        return 0.0;
    }
    let length = (track * port_span / (port_span + max_offset)).clamp(THUMB_MIN.min(track), track);
    let travel = (track - length).max(0.0);
    if travel <= 0.0 {
        return 0.0;
    }
    let along = (pointer - grab - port_origin - THUMB_INSET).clamp(0.0, travel);
    along / travel * max_offset
}

fn thumb_along(port: Rect, offset: f32, max_offset: f32, vertical: bool) -> Option<Rect> {
    if max_offset <= 0.0 {
        return None;
    }
    let track_len = if vertical { port.height } else { port.width };
    let track = track_len - THUMB_INSET * 2.0;
    if track <= 0.0 {
        return None;
    }
    let content = track_len + max_offset;
    let length = (track * track_len / content).clamp(THUMB_MIN.min(track), track);
    let travel = (track - length).max(0.0);
    let position = THUMB_INSET + travel * (offset / max_offset).clamp(0.0, 1.0);
    if vertical {
        Some(Rect::new(
            port.right() - THUMB_INSET - THUMB_THICKNESS,
            port.y + position,
            THUMB_THICKNESS,
            length,
        ))
    } else {
        Some(Rect::new(
            port.x + position,
            port.bottom() - THUMB_INSET - THUMB_THICKNESS,
            length,
            THUMB_THICKNESS,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_stops_on_the_target_and_never_passes_it() {
        let mut offset = 0.0;
        let target = 40.0;
        offset = ease_toward(offset, target, 0.0);
        assert_eq!(offset, 0.0);
        offset = ease_toward(offset, target, 10.0);
        assert_eq!(offset, target);

        offset = 0.0;
        for _ in 0..40 {
            let next = ease_toward(offset, target, 0.016);
            assert!(next >= offset && next <= target);
            offset = next;
        }
        assert_eq!(offset, target);
    }

    #[test]
    fn an_idle_gap_does_not_advance_the_ease() {
        let start = Instant::now();
        assert_eq!(scroll_dt(None, start), 0.0);
        assert_eq!(
            scroll_dt(Some(start), start + Duration::from_millis(16)),
            0.016
        );
        assert_eq!(scroll_dt(Some(start), start + Duration::from_secs(2)), 0.0);
    }

    #[test]
    fn grabbing_the_thumb_maps_back_to_the_same_offset() {
        let port = Rect::new(10.0, 20.0, 120.0, 80.0);
        let max = 120.0;
        let thumb = vertical_thumb(port, 0.0, max).unwrap();
        let pointer = thumb.y + thumb.height * 0.5;
        let grab = pointer - thumb.y;
        let again = offset_for_grab(port.y, port.height, pointer, grab, max);
        assert!(again.abs() < 0.01, "{again}");

        let jumped = offset_for_grab(
            port.y,
            port.height,
            port.bottom() - 4.0,
            thumb.height * 0.5,
            max,
        );
        assert!(jumped > 0.0 && jumped <= max);

        let past = offset_for_grab(port.y, port.height, port.bottom() + 400.0, grab, max);
        assert!((past - max).abs() < 0.01);
    }
}
