//! Scroll offset easing and scrollbar geometry.
//!
//! A wheel event moves the target; the offset chases it and stops there. Dragging the scrollbar
//! writes the offset directly so the thumb stays under the pointer.

use crate::time::{Duration, Instant};

use cappy_layout::{Point, Rect};
use cappy_style::{Color, Edges, Insets};

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

/// The box children are clipped to: the border box inset by each painted border edge.
pub(crate) fn scrollport(rect: Rect, border: Edges) -> Rect {
    let left = border.left.max(0.0);
    let top = border.top.max(0.0);
    let right = border.right.max(0.0);
    let bottom = border.bottom.max(0.0);
    Rect::new(
        rect.x + left,
        rect.y + top,
        (rect.width - left - right).max(0.0),
        (rect.height - top - bottom).max(0.0),
    )
}

/// Visual rect of a sticky item. `static_rect` is the in-flow position, already shifted by scroll.
///
/// Each set inset sticks to that edge of its scrollport. The start inset wins when the start and
/// end cannot both hold. The result then stays inside `parent_box`, so the item leaves with its
/// parent. A missing port leaves that axis on the static position before the parent clamp.
pub(crate) fn sticky_rect(
    static_rect: Rect,
    parent_box: Rect,
    port_x: Option<Rect>,
    port_y: Option<Rect>,
    inset: Insets,
) -> Rect {
    let mut x = static_rect.x;
    let mut y = static_rect.y;
    if let Some(port) = port_y {
        if let Some(top) = inset.top {
            y = y.max(port.y + top);
        }
        if let Some(bottom) = inset.bottom {
            let limit = port.bottom() - static_rect.height - bottom;
            y = if let Some(top) = inset.top {
                y.min(limit.max(port.y + top))
            } else {
                y.min(limit)
            };
        }
    }
    if let Some(port) = port_x {
        if let Some(left) = inset.left {
            x = x.max(port.x + left);
        }
        if let Some(right) = inset.right {
            let limit = port.right() - static_rect.width - right;
            x = if let Some(left) = inset.left {
                x.min(limit.max(port.x + left))
            } else {
                x.min(limit)
            };
        }
    }
    let max_x = (parent_box.right() - static_rect.width).max(parent_box.x);
    let max_y = (parent_box.bottom() - static_rect.height).max(parent_box.y);
    Rect::new(
        x.clamp(parent_box.x, max_x),
        y.clamp(parent_box.y, max_y),
        static_rect.width,
        static_rect.height,
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

    #[test]
    fn a_sticky_box_stays_inside_its_parent_when_the_parent_leaves() {
        let parent = Rect::new(0.0, -30.0, 100.0, 40.0);
        let port = Rect::new(0.0, 0.0, 100.0, 80.0);
        let stuck = sticky_rect(
            Rect::new(0.0, -30.0, 100.0, 20.0),
            parent,
            None,
            Some(port),
            Insets {
                top: Some(0.0),
                ..Insets::default()
            },
        );
        assert_eq!(stuck.y, -10.0);

        let held = sticky_rect(
            Rect::new(0.0, -10.0, 100.0, 20.0),
            Rect::new(0.0, -10.0, 100.0, 80.0),
            None,
            Some(port),
            Insets {
                top: Some(0.0),
                ..Insets::default()
            },
        );
        assert_eq!(held.y, 0.0);
    }

    #[test]
    fn the_start_inset_wins_when_both_edges_cannot_hold() {
        let port = Rect::new(0.0, 0.0, 100.0, 40.0);
        let stuck = sticky_rect(
            Rect::new(-5.0, 30.0, 20.0, 50.0),
            Rect::new(0.0, 0.0, 100.0, 200.0),
            Some(port),
            Some(port),
            Insets {
                top: Some(0.0),
                bottom: Some(0.0),
                left: Some(0.0),
                right: None,
            },
        );
        assert_eq!(stuck.y, 0.0);
        assert_eq!(stuck.x, 0.0);
    }
}
