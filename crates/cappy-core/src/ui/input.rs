//! Pointer, keyboard and focus handling.

use cappy_layout::{Point, Rect, TextMeasureRequest};
use cappy_style::{Display, InteractionState, Overflow};

use super::edit::{EditChange, TextEdit};
use super::node::NodeContent;
use super::scroll::{
    ScrollAxis, axis_clip, horizontal_thumb, offset_for_grab, scrollbar_track, scrollport,
    vertical_thumb,
};
use super::{NodeId, Ui};
use crate::time::{Duration, Instant};
use crate::{
    ButtonState, ClickEvent, ImeEvent, InputEvent, Key, KeyEvent, PointerButton, PointerEvent,
    PointerPhase,
};

const CARET_BLINK: Duration = Duration::from_millis(530);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

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
                    self.drag_capture(PointerPhase::Move);
                    self.refresh_hover();
                }
            }
            InputEvent::PointerLeft => {
                self.pointer = None;
                self.blur();
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
            InputEvent::Modifiers(modifiers) => self.modifiers = modifiers,
            InputEvent::Key(event) => self.on_key(event),
            InputEvent::Text(text) => self.on_text(&text),
            InputEvent::Ime(event) => self.on_ime(event),
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
        let focus_target = self.hovered.iter().rev().copied().find(|id| {
            self.is_current(*id) && self.nodes[id.index()].focusable && !self.node_disabled(*id)
        });
        if let Some(id) = focus_target {
            let count = self.press_count(id);
            self.focus(id);
            self.pointer_capture = Some(id);
            self.pointer_down(id, count);
        } else {
            self.blur();
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
        if let Some(id) = self.pointer_capture.take()
            && self.is_current(id)
        {
            self.emit_pointer(id, PointerPhase::Up, 1);
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

impl Ui {
    fn node_disabled(&self, id: NodeId) -> bool {
        match &self.nodes[id.index()].content {
            NodeContent::Input(field) => field.disabled,
            _ => self.nodes[id.index()].disabled,
        }
    }

    fn press_count(&mut self, id: NodeId) -> u8 {
        let now = Instant::now();
        let count = if self.last_press.is_some_and(|(previous, at)| {
            previous == id && now.saturating_duration_since(at) < DOUBLE_CLICK
        }) {
            2
        } else {
            1
        };
        self.last_press = Some((id, now));
        count
    }

    fn pointer_down(&mut self, id: NodeId, count: u8) {
        if self.is_input(id) {
            let index = self.input_hit(id);
            let extend = self.modifiers.shift && count < 2;
            if count >= 2 {
                self.edit_input(id, |edit| edit.select_word(index));
            } else {
                self.edit_input(id, |edit| edit.place(index, extend));
            }
            self.reveal_input_caret(id);
            self.show_caret(id);
        }
        self.emit_pointer(id, PointerPhase::Down, count);
    }

    fn drag_capture(&mut self, phase: PointerPhase) {
        let Some(id) = self.pointer_capture else {
            return;
        };
        if !self.is_current(id) {
            self.pointer_capture = None;
            return;
        }
        if self.is_input(id) && phase == PointerPhase::Move {
            let index = self.input_hit(id);
            self.edit_input(id, |edit| edit.place(index, true));
            self.reveal_input_caret(id);
            self.show_caret(id);
        }
        self.emit_pointer(id, phase, 1);
    }

    fn emit_pointer(&mut self, id: NodeId, phase: PointerPhase, click_count: u8) {
        let Some(position) = self.pointer else {
            return;
        };
        let text_index = self.pointer_text_index(id, position);
        let shift = self.modifiers.shift;
        let mut handler = self.nodes[id.index()].on_pointer.take();
        if let Some(handler) = handler.as_mut() {
            handler(&PointerEvent {
                position,
                phase,
                text_index,
                shift,
                click_count,
            });
        }
        if self.is_current(id) {
            self.nodes[id.index()].on_pointer = handler;
        }
    }

    fn pointer_text_index(&mut self, id: NodeId, point: Point) -> Option<usize> {
        if self.is_input(id) {
            return Some(self.input_hit(id));
        }
        let text = self.hit_path(point).into_iter().rev().find(|candidate| {
            self.is_current(*candidate)
                && self.nodes[candidate.index()].is_text()
                && self.is_within(*candidate, id)
        })?;
        Some(self.text_hit(text, point))
    }

    fn is_within(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node) = current {
            if node == ancestor {
                return true;
            }
            current = self.nodes[node.index()].parent;
        }
        false
    }

    fn on_key(&mut self, event: KeyEvent) {
        self.modifiers = event.modifiers;
        if let Some(id) = self.focused {
            self.nodes.get_mut(id.index()).map(|node| {
                node.content = match std::mem::replace(&mut node.content, NodeContent::Container) {
                    NodeContent::Input(mut field) => {
                        field.swallow_text = None;
                        NodeContent::Input(field)
                    }
                    other => other,
                };
            });
        }
        if event.state == ButtonState::Pressed && event.key == Key::Tab {
            self.focus_step(event.modifiers.shift);
            return;
        }
        let Some(id) = self.focused else {
            return;
        };
        if !self.is_current(id) || self.node_disabled(id) {
            return;
        }
        if self.is_input(id) && event.state == ButtonState::Pressed {
            self.input_key(id, event);
        }
        self.deliver_key(id, event);
    }

    fn on_text(&mut self, text: &str) {
        let Some(id) = self.focused else {
            return;
        };
        if !self.is_current(id) || self.node_disabled(id) {
            return;
        }
        if self.is_input(id) {
            let swallowed = matches!(
                &self.nodes[id.index()].content,
                NodeContent::Input(field) if field.swallow_text.as_deref() == Some(text)
            );
            if swallowed {
                if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
                    field.swallow_text = None;
                }
            } else if self.edit_input(id, |edit| edit.insert(text)) == EditChange::Value {
                self.publish_input(id);
                self.reveal_input_caret(id);
                self.show_caret(id);
            }
        }
        let mut handler = self.nodes[id.index()].on_text.take();
        if let Some(handler) = handler.as_mut() {
            handler(text);
        }
        if self.is_current(id) {
            self.nodes[id.index()].on_text = handler;
        }
    }

    fn on_ime(&mut self, event: ImeEvent) {
        let Some(id) = self.focused else {
            return;
        };
        if !self.is_current(id) || self.node_disabled(id) {
            return;
        }
        if self.is_input(id) {
            let change = match &event {
                ImeEvent::Preedit { text, cursor } => {
                    self.edit_input(id, |edit| edit.set_preedit(text, *cursor))
                }
                ImeEvent::Commit(text) => {
                    let change = self.edit_input(id, |edit| edit.insert(text));
                    if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
                        field.swallow_text = Some(text.clone());
                    }
                    change
                }
            };
            if change == EditChange::Value {
                self.publish_input(id);
            }
            if change != EditChange::None {
                self.reveal_input_caret(id);
                self.show_caret(id);
            }
        }
        let mut handler = self.nodes[id.index()].on_ime.take();
        if let Some(handler) = handler.as_mut() {
            handler(&event);
        }
        if self.is_current(id) {
            self.nodes[id.index()].on_ime = handler;
        }
    }

    fn input_key(&mut self, id: NodeId, event: KeyEvent) {
        let shift = event.modifiers.shift;
        let word = event.modifiers.primary();
        let change = match event.key {
            Key::Backspace => self.edit_input(id, |edit| edit.backspace(word)),
            Key::Delete => self.edit_input(id, |edit| edit.delete_forward(word)),
            Key::Left => self.edit_input(id, |edit| edit.move_left(shift, word)),
            Key::Right => self.edit_input(id, |edit| edit.move_right(shift, word)),
            Key::Home => self.edit_input(id, |edit| edit.move_home(shift)),
            Key::End => self.edit_input(id, |edit| edit.move_end(shift)),
            Key::Enter => {
                self.submit_input(id);
                return;
            }
            Key::Character(ch) if event.modifiers.primary() => match ch.to_ascii_lowercase() {
                'a' => self.edit_input(id, TextEdit::select_all),
                'c' => {
                    self.copy_input(id);
                    return;
                }
                'x' => {
                    self.cut_input(id);
                    return;
                }
                'v' => {
                    self.paste_input(id);
                    return;
                }
                'z' if shift => self.edit_input(id, TextEdit::redo),
                'z' => self.edit_input(id, TextEdit::undo),
                'y' => self.edit_input(id, TextEdit::redo),
                _ => EditChange::None,
            },
            _ => EditChange::None,
        };
        if change == EditChange::Value {
            self.publish_input(id);
        }
        if change != EditChange::None {
            self.reveal_input_caret(id);
            self.show_caret(id);
        }
    }

    fn focus_step(&mut self, reverse: bool) {
        let order = self.focus_order();
        if order.is_empty() {
            return;
        }
        let next = match self
            .focused
            .and_then(|id| order.iter().position(|item| *item == id))
        {
            Some(index) if reverse => {
                if index == 0 {
                    order.len() - 1
                } else {
                    index - 1
                }
            }
            Some(index) => (index + 1) % order.len(),
            None if reverse => order.len() - 1,
            None => 0,
        };
        self.focus(order[next]);
    }

    fn focus_order(&self) -> Vec<NodeId> {
        let mut order = Vec::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if !self.is_current(id) {
                continue;
            }
            let node = &self.nodes[id.index()];
            if node.suppressed || node.style.layout.display == Display::None {
                continue;
            }
            if node.focusable && !self.node_disabled(id) {
                order.push(id);
            }
            stack.extend(node.children.iter().rev().copied());
        }
        order
    }

    pub(super) fn focus(&mut self, id: NodeId) {
        if self.focused == Some(id) || !self.is_focusable(id) {
            return;
        }
        self.blur();
        self.focused = Some(id);
        self.set_interaction(id, |state| state.focused = true);
        self.call_focus(id, true);
        self.show_caret(id);
    }

    pub(super) fn blur(&mut self) {
        let Some(id) = self.focused.take() else {
            return;
        };
        self.caret_deadline = None;
        if self.is_current(id) {
            self.set_interaction(id, |state| state.focused = false);
            self.call_focus(id, false);
            self.mark_paint_dirty(id);
        }
    }

    fn is_focusable(&self, id: NodeId) -> bool {
        self.is_current(id)
            && self.nodes[id.index()].focusable
            && !self.node_disabled(id)
            && !self.nodes[id.index()].suppressed
            && self.nodes[id.index()].style.layout.display != Display::None
    }

    fn call_focus(&mut self, id: NodeId, focused: bool) {
        let mut handler = self.nodes[id.index()].on_focus.take();
        if let Some(handler) = handler.as_mut() {
            handler(focused);
        }
        if self.is_current(id) {
            self.nodes[id.index()].on_focus = handler;
        }
    }

    fn deliver_key(&mut self, id: NodeId, event: KeyEvent) {
        let mut handler = self.nodes[id.index()].on_key.take();
        if let Some(handler) = handler.as_mut() {
            handler(&event, self.clipboard.as_mut());
        }
        if self.is_current(id) {
            self.nodes[id.index()].on_key = handler;
        }
    }

    fn is_input(&self, id: NodeId) -> bool {
        self.is_current(id) && matches!(self.nodes[id.index()].content, NodeContent::Input(_))
    }

    fn edit_input(
        &mut self,
        id: NodeId,
        edit: impl FnOnce(&mut TextEdit) -> EditChange,
    ) -> EditChange {
        let NodeContent::Input(field) = &mut self.nodes[id.index()].content else {
            return EditChange::None;
        };
        edit(&mut field.edit)
    }

    fn copy_input(&mut self, id: NodeId) {
        let Some(text) = self.input_selection(id) else {
            return;
        };
        self.clipboard.write_text(&text);
    }

    fn cut_input(&mut self, id: NodeId) {
        let NodeContent::Input(field) = &mut self.nodes[id.index()].content else {
            return;
        };
        let Some(text) = field.edit.cut() else {
            return;
        };
        self.clipboard.write_text(&text);
        self.publish_input(id);
        self.reveal_input_caret(id);
        self.show_caret(id);
    }

    fn paste_input(&mut self, id: NodeId) {
        let Some(text) = self.clipboard.read_text() else {
            return;
        };
        if self.edit_input(id, |edit| edit.insert(&text)) == EditChange::Value {
            self.publish_input(id);
            self.reveal_input_caret(id);
            self.show_caret(id);
        }
    }

    fn input_selection(&self, id: NodeId) -> Option<String> {
        let NodeContent::Input(field) = &self.nodes[id.index()].content else {
            return None;
        };
        field.edit.selected_text()
    }

    fn submit_input(&mut self, id: NodeId) {
        let text = self.input_text(id);
        let mut handler = match &mut self.nodes[id.index()].content {
            NodeContent::Input(field) => field.on_submit.take(),
            _ => None,
        };
        if let Some(handler) = handler.as_mut() {
            handler(&text);
        }
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            field.on_submit = handler;
        }
    }

    fn publish_input(&mut self, id: NodeId) {
        let text = self.input_text(id);
        let writer = match &mut self.nodes[id.index()].content {
            NodeContent::Input(field) if field.published != text => {
                field.published = text.clone();
                field.writer.take()
            }
            _ => None,
        };
        if let Some(writer) = writer.as_ref() {
            writer(text.clone());
        }
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            if field.writer.is_none() {
                field.writer = writer;
            }
        }
        let mut handler = match &mut self.nodes[id.index()].content {
            NodeContent::Input(field) => field.on_input.take(),
            _ => None,
        };
        if let Some(handler) = handler.as_mut() {
            handler(&text);
        }
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            field.on_input = handler;
        }
        self.mark_paint_dirty(id);
    }

    fn input_text(&self, id: NodeId) -> String {
        match &self.nodes[id.index()].content {
            NodeContent::Input(field) => field.edit.text().to_owned(),
            _ => String::new(),
        }
    }

    pub(super) fn apply_input_value(&mut self, id: NodeId, value: String) {
        if !self.is_input(id) {
            return;
        }
        let focused = self.focused == Some(id);
        let unchanged = matches!(
            &self.nodes[id.index()].content,
            NodeContent::Input(field) if field.edit.text() == value
        );
        if unchanged {
            return;
        }
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            field.edit.set_text(value.clone(), focused);
            field.published = value;
        }
        self.reveal_input_caret(id);
        self.mark_paint_dirty(id);
    }

    pub(super) fn apply_input_disabled(&mut self, id: NodeId, disabled: bool) {
        let NodeContent::Input(field) = &mut self.nodes[id.index()].content else {
            return;
        };
        if field.disabled == disabled {
            return;
        }
        field.disabled = disabled;
        if disabled && self.focused == Some(id) {
            self.blur();
        }
    }

    pub(super) fn apply_text_caret(&mut self, id: NodeId, caret: Option<usize>) {
        if !self.is_current(id) || self.nodes[id.index()].caret == caret {
            return;
        }
        self.nodes[id.index()].caret = caret;
        self.mark_paint_dirty(id);
    }

    pub(super) fn apply_text_selection(&mut self, id: NodeId, selection: Option<(usize, usize)>) {
        if !self.is_current(id) || self.nodes[id.index()].selection == selection {
            return;
        }
        self.nodes[id.index()].selection = selection;
        self.mark_paint_dirty(id);
    }

    fn input_hit(&mut self, id: NodeId) -> usize {
        let Some(point) = self.pointer else {
            return 0;
        };
        let local = self.local_point(id, point);
        let (display, scroll, origin, text_style) = {
            let node = &self.nodes[id.index()];
            let NodeContent::Input(field) = &node.content else {
                return 0;
            };
            if field.edit.text().is_empty() && field.edit.display().text.is_empty() {
                return 0;
            }
            (
                field.edit.display(),
                field.scroll_x,
                content_box(node),
                node.style.text.clone(),
            )
        };
        let x = local.x - origin.x + scroll;
        let y = local.y - origin.y;
        self.measurer.hit_test(
            &TextMeasureRequest {
                text: &display.text,
                font_size: text_style.font_size,
                line_height: text_style.line_height_px(),
                weight: text_style.weight.0,
            },
            None,
            x,
            y,
        )
    }

    fn text_hit(&mut self, id: NodeId, point: Point) -> usize {
        let local = self.local_point(id, point);
        let (text, origin_rect, text_style) = {
            let node = &self.nodes[id.index()];
            let Some(text) = node.text() else {
                return 0;
            };
            (text.to_owned(), content_box(node), node.style.text.clone())
        };
        let width = origin_rect.width;
        self.measurer.hit_test(
            &TextMeasureRequest {
                text: &text,
                font_size: text_style.font_size,
                line_height: text_style.line_height_px(),
                weight: text_style.weight.0,
            },
            Some(width),
            local.x - origin_rect.x,
            local.y - origin_rect.y,
        )
    }

    fn reveal_input_caret(&mut self, id: NodeId) {
        if !self.is_input(id) {
            return;
        }
        let (prefix, scroll, view, text_style) = {
            let node = &self.nodes[id.index()];
            let NodeContent::Input(field) = &node.content else {
                return;
            };
            let display = field.edit.display();
            let caret = display.caret.min(display.text.len());
            (
                display.text.get(..caret).unwrap_or("").to_owned(),
                field.scroll_x,
                content_box(node).width,
                node.style.text.clone(),
            )
        };
        let width = self
            .measurer
            .measure(
                &TextMeasureRequest {
                    text: &prefix,
                    font_size: text_style.font_size,
                    line_height: text_style.line_height_px(),
                    weight: text_style.weight.0,
                },
                None,
            )
            .width;
        let next = if width < scroll {
            width
        } else if width > scroll + view {
            (width - view).max(0.0)
        } else {
            scroll
        };
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content
            && (field.scroll_x - next).abs() > f32::EPSILON
        {
            field.scroll_x = next;
            self.mark_paint_dirty(id);
        }
    }

    fn show_caret(&mut self, id: NodeId) {
        self.caret_on = true;
        self.caret_deadline = Some(Instant::now() + CARET_BLINK);
        if self.is_current(id) {
            self.mark_paint_dirty(id);
        }
    }

    pub(super) fn step_caret(&mut self, now: Instant) {
        let Some(deadline) = self.caret_deadline else {
            return;
        };
        if self.focused.is_none() || now < deadline {
            return;
        }
        self.caret_on = !self.caret_on;
        self.caret_deadline = Some(now + CARET_BLINK);
        if let Some(id) = self.focused
            && self.is_current(id)
        {
            self.mark_paint_dirty(id);
        }
    }
}

fn content_box(node: &super::node::Node) -> Rect {
    let padding = node.style.layout.padding;
    Rect::new(
        node.rect.x + padding.left,
        node.rect.y + padding.top,
        (node.rect.width - padding.horizontal()).max(0.0),
        (node.rect.height - padding.vertical()).max(0.0),
    )
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
