//! Pointer handling: hit testing, hover/pressed state and click dispatch.

use cappy_layout::{Point, Rect};
use cappy_style::{Display, InteractionState, Overflow};

use super::scroll::{
    ScrollAxis, axis_clip, horizontal_thumb, offset_for_grab, scrollbar_track, scrollport,
    vertical_thumb,
};
use super::{NodeId, Ui};
use crate::{ButtonState, ClickEvent, InputEvent, PointerButton};

/// A press on a scrollbar thumb or its track, held until the button is released.
#[derive(Clone, Copy, Debug)]
pub(super) struct ScrollbarDrag {
    pub(super) id: NodeId,
    axis: ScrollAxis,
    /// Distance from the thumb's leading edge to the pointer.
    grab: f32,
}

impl Ui {
    /// Feeds one input event. Call [`Ui::update`] afterwards if [`Ui::needs_update`] is true.
    pub fn handle_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::PointerMoved { position } => {
                self.pointer = Some(position);
                if self.scrollbar_drag.is_some() {
                    self.drag_scrollbar(position);
                } else {
                    self.refresh_hover();
                }
            }
            InputEvent::PointerLeft => {
                self.pointer = None;
                if self.scrollbar_drag.is_none() {
                    self.refresh_hover();
                }
            }
            InputEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            } => match state {
                ButtonState::Pressed => self.press(),
                ButtonState::Released => self.release(),
            },
            InputEvent::PointerButton { .. } => {}
            InputEvent::Wheel { delta } => self.wheel(delta),
        }
    }

    /// Applies `delta` to the deepest scroll container that can move, then to its parents.
    /// Positive delta moves content down and right, which decreases the scroll offset.
    fn wheel(&mut self, mut delta: Point) {
        let Some(position) = self.pointer else {
            return;
        };
        let path = self.hit_path(position);
        for id in path.into_iter().rev() {
            if delta.x == 0.0 && delta.y == 0.0 {
                break;
            }
            delta = self.wheel_node(id, delta);
        }
    }

    /// Returns the portion of `delta` this node did not use.
    fn wheel_node(&mut self, id: NodeId, delta: Point) -> Point {
        if !self.is_current(id) {
            return delta;
        }
        let max = self.scroll_max(id);
        let layout = &self.nodes[id.index()].style.layout;
        let target = self.nodes[id.index()].scroll_target;
        let (next_x, used_x) = consume_axis(target.x, delta.x, max.x, layout.overflow_x.scrolls());
        let (next_y, used_y) = consume_axis(target.y, delta.y, max.y, layout.overflow_y.scrolls());
        if next_x != target.x || next_y != target.y {
            self.nodes[id.index()].scroll_target = Point::new(next_x, next_y);
        }
        Point::new(delta.x - used_x, delta.y - used_y)
    }

    /// Path from the root to the deepest visible node under `point`.
    pub fn hit_path(&self, point: Point) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = Some(self.root).filter(|root| self.is_hit(*root, point));
        while let Some(id) = current {
            path.push(id);
            current = self.nodes[id.index()]
                .children
                .iter()
                .rev()
                .copied()
                .find(|child| self.is_hit(*child, point));
        }
        path
    }

    fn is_hit(&self, id: NodeId, point: Point) -> bool {
        let Some(node) = self.nodes.get(id.index()) else {
            return false;
        };
        node.generation == id.generation()
            && node.alive
            && !node.suppressed
            && node.style.layout.display != Display::None
            && node.rect.contains(self.local_point(id, point))
            && self.inside_ancestor_clips(id, point)
    }

    /// Maps a visual point back into `id`'s layout rect. Identity scale and rotation leave it.
    fn local_point(&self, id: NodeId, point: Point) -> Point {
        let Some(owner) = self.transform_owner(id) else {
            return point;
        };
        let (scale, rotation) = self.accumulated_spin(owner, id);
        crate::paint::unspin_point(
            point,
            crate::paint::rect_center(self.nodes[owner.index()].rect),
            scale,
            rotation,
        )
    }

    fn transform_owner(&self, id: NodeId) -> Option<NodeId> {
        let mut current = Some(id);
        let mut owner = None;
        while let Some(node_id) = current {
            let node = &self.nodes[node_id.index()];
            if (node.scale - 1.0).abs() > 1.0e-4 || node.rotation.abs() > 1.0e-4 {
                owner = Some(node_id);
            }
            current = node.parent;
        }
        owner
    }

    fn accumulated_spin(&self, owner: NodeId, id: NodeId) -> (f32, f32) {
        let mut scale = 1.0;
        let mut rotation = 0.0;
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = &self.nodes[node_id.index()];
            scale *= node.scale;
            rotation += node.rotation;
            if node_id == owner {
                break;
            }
            current = node.parent;
        }
        (scale, rotation)
    }

    fn inside_ancestor_clips(&self, id: NodeId, point: Point) -> bool {
        let mut current = self.nodes[id.index()].parent;
        while let Some(parent) = current {
            let node = &self.nodes[parent.index()];
            if node.style.layout.clips() {
                let clip = axis_clip(
                    scrollport(node.rect, node.style.visual.border_width),
                    node.style.layout.overflow_x.clips(),
                    node.style.layout.overflow_y.clips(),
                );
                if !clip.contains(self.local_point(parent, point)) {
                    return false;
                }
            }
            current = node.parent;
        }
        true
    }

    pub(super) fn refresh_hover(&mut self) {
        let path = self
            .pointer
            .map(|point| self.hit_path(point))
            .unwrap_or_default();
        if path == self.hovered {
            return;
        }
        let previous = std::mem::replace(&mut self.hovered, path.clone());
        let left: Vec<NodeId> = previous
            .iter()
            .copied()
            .filter(|id| !path.contains(id) && self.is_current(*id))
            .collect();
        let entered: Vec<NodeId> = path
            .iter()
            .copied()
            .filter(|id| !previous.contains(id))
            .collect();
        for id in left {
            self.set_interaction(id, |state| state.hovered = false);
            self.call_hover_handler(id, false);
        }
        for id in entered {
            self.set_interaction(id, |state| state.hovered = true);
            self.call_hover_handler(id, true);
        }
    }

    fn press(&mut self) {
        if let Some(position) = self.pointer
            && let Some(drag) = self.scrollbar_at(position)
        {
            self.scrollbar_drag = Some(drag);
            self.drag_scrollbar(position);
            return;
        }
        let target = self
            .hovered
            .iter()
            .rev()
            .copied()
            .find(|id| self.is_current(*id) && self.nodes[id.index()].on_click.is_some());
        if let Some(id) = target {
            self.pressed = Some(id);
            self.set_interaction(id, |state| state.pressed = true);
        }
    }

    fn release(&mut self) {
        if self.scrollbar_drag.take().is_some() {
            self.refresh_hover();
            return;
        }
        let Some(id) = self.pressed.take() else {
            return;
        };
        if !self.is_current(id) {
            return;
        }
        self.set_interaction(id, |state| state.pressed = false);
        if self.hovered.contains(&id) {
            let event = ClickEvent {
                position: self.pointer.unwrap_or_default(),
            };
            let node = &mut self.nodes[id.index()];
            if let Some(mut handler) = node.on_click.take() {
                handler(&event);
                node.on_click.get_or_insert(handler);
            }
        }
    }

    pub(super) fn is_current(&self, id: NodeId) -> bool {
        self.nodes
            .get(id.index())
            .is_some_and(|node| node.alive && node.generation == id.generation())
    }

    pub(super) fn set_interaction(
        &mut self,
        id: NodeId,
        change: impl FnOnce(&mut InteractionState),
    ) {
        if !self.is_current(id) {
            return;
        }
        let node = &mut self.nodes[id.index()];
        let before = node.interaction;
        change(&mut node.interaction);
        if node.interaction != before {
            self.mark_style_dirty(id);
        }
    }

    /// Deepest scrollbar whose track contains `point`.
    fn scrollbar_at(&self, point: Point) -> Option<ScrollbarDrag> {
        self.hit_path(point)
            .into_iter()
            .rev()
            .find_map(|id| self.scrollbar_grab(id, point))
    }

    fn scrollbar_grab(&self, id: NodeId, point: Point) -> Option<ScrollbarDrag> {
        if !self.is_current(id) {
            return None;
        }
        let (port, overflow_x, overflow_y, scroll) = {
            let node = &self.nodes[id.index()];
            (
                scrollport(node.rect, node.style.visual.border_width),
                node.style.layout.overflow_x,
                node.style.layout.overflow_y,
                node.scroll,
            )
        };
        let max = self.scroll_max(id);
        let axis = scrollbar_axis(overflow_x, overflow_y, port, max, scroll, point)?;
        let (offset, max_offset) = match axis {
            ScrollAxis::Vertical => (scroll.y, max.y),
            ScrollAxis::Horizontal => (scroll.x, max.x),
        };
        let thumb = match axis {
            ScrollAxis::Vertical => vertical_thumb(port, offset, max_offset)?,
            ScrollAxis::Horizontal => horizontal_thumb(port, offset, max_offset)?,
        };
        let (thumb_origin, length, pointer) = match axis {
            ScrollAxis::Vertical => (thumb.y, thumb.height, point.y),
            ScrollAxis::Horizontal => (thumb.x, thumb.width, point.x),
        };
        let on_thumb = pointer >= thumb_origin && pointer < thumb_origin + length;
        let grab = if on_thumb {
            pointer - thumb_origin
        } else {
            length * 0.5
        };
        Some(ScrollbarDrag { id, axis, grab })
    }

    fn drag_scrollbar(&mut self, point: Point) {
        let Some(drag) = self.scrollbar_drag else {
            return;
        };
        if !self.is_current(drag.id) {
            self.scrollbar_drag = None;
            return;
        }
        let (rect, border) = {
            let node = &self.nodes[drag.id.index()];
            (node.rect, node.style.visual.border_width)
        };
        let port = scrollport(rect, border);
        let max = self.scroll_max(drag.id);
        let (origin, span, pointer, max_offset, vertical) = match drag.axis {
            ScrollAxis::Vertical => (port.y, port.height, point.y, max.y, true),
            ScrollAxis::Horizontal => (port.x, port.width, point.x, max.x, false),
        };
        let offset = offset_for_grab(origin, span, pointer, drag.grab, max_offset);
        self.jump_scroll(drag.id, vertical, offset);
    }

    fn call_hover_handler(&mut self, id: NodeId, hovered: bool) {
        if !self.is_current(id) {
            return;
        }
        let node = &mut self.nodes[id.index()];
        if let Some(mut handler) = node.on_hover.take() {
            handler(hovered);
            node.on_hover.get_or_insert(handler);
        }
    }
}

fn scrollbar_axis(
    overflow_x: Overflow,
    overflow_y: Overflow,
    port: Rect,
    max: Point,
    scroll: Point,
    point: Point,
) -> Option<ScrollAxis> {
    let vertical = overflow_y.scrolls()
        && max.y > 0.0
        && scrollbar_track(port, ScrollAxis::Vertical).contains(point);
    let horizontal = overflow_x.scrolls()
        && max.x > 0.0
        && scrollbar_track(port, ScrollAxis::Horizontal).contains(point);
    match (vertical, horizontal) {
        (false, false) => None,
        (true, false) => Some(ScrollAxis::Vertical),
        (false, true) => Some(ScrollAxis::Horizontal),
        (true, true) => {
            let on_vertical =
                vertical_thumb(port, scroll.y, max.y).is_some_and(|thumb| thumb.contains(point));
            let on_horizontal =
                horizontal_thumb(port, scroll.x, max.x).is_some_and(|thumb| thumb.contains(point));
            Some(match (on_vertical, on_horizontal) {
                (true, false) => ScrollAxis::Vertical,
                (false, true) => ScrollAxis::Horizontal,
                _ if port.right() - point.x <= port.bottom() - point.y => ScrollAxis::Vertical,
                _ => ScrollAxis::Horizontal,
            })
        }
    }
}

/// `delta` is content movement. Positive moves content down, which decreases the scroll offset.
/// The second value is how much of `delta` was consumed.
fn consume_axis(target: f32, delta: f32, max: f32, scrolls: bool) -> (f32, f32) {
    if !scrolls {
        return (target, 0.0);
    }
    let next = (target - delta).clamp(0.0, max.max(0.0));
    (next, target - next)
}
