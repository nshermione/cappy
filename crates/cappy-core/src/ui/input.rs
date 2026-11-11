//! Pointer, keyboard and focus handling.

use cappy_layout::{Point, Rect, TextMeasureRequest};
use cappy_style::{Display, InteractionState, Overflow, TextAlign};

use super::edit::{EditChange, TextEdit};
use super::node::NodeContent;
use super::scroll::{
    ScrollAxis, axis_clip, horizontal_thumb, offset_for_grab, scrollbar_track, scrollport,
    vertical_thumb,
};
use super::{NodeId, Ui};
use crate::time::{Duration, Instant};
use crate::{
    ButtonState, ClickEvent, FocusScope, ImeEvent, InputEvent, Key, KeyEvent, PointerButton,
    PointerEvent, PointerPhase,
};

const CARET_BLINK: Duration = Duration::from_millis(530);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Deepest focusable node under the pointer, or why there is none.
enum PressFocus {
    Target(NodeId),
    /// A disabled focusable node is in front. Do not focus or press through it.
    Blocked,
    None,
}

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
        let _entered = self.queue.runtime.enter_board(&self.board);
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
                self.button_capture = None;
                self.pending_dismiss.clear();
                self.finish_deferred_blur();
                self.blur();
                if self.scrollbar_drag.is_none() {
                    self.refresh_hover();
                }
            }
            InputEvent::PointerButton { button, state } => match button {
                PointerButton::Primary => match state {
                    ButtonState::Pressed => self.press(),
                    ButtonState::Released => self.release(),
                },
                PointerButton::Secondary | PointerButton::Middle => {
                    self.other_button(button, state)
                }
            },
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

    /// Path from the hit root to the deepest visible node under `point`.
    ///
    /// An overlay is tested before the page, topmost first. A hit there does not include the
    /// page nodes underneath, so a wheel on the overlay cannot scroll them. A miss falls through
    /// to the next overlay and then to the page. Overlay nodes are skipped while walking the page.
    pub fn hit_path(&self, point: Point) -> Vec<NodeId> {
        for id in self.overlay_layers().into_iter().rev() {
            if self.layer_shown(id)
                && let Some(path) = self.hit_chain(id, point)
            {
                return path;
            }
        }
        self.hit_chain(self.root, point).unwrap_or_default()
    }

    /// `root` to the deepest in-flow descendant under `point`. Overlay children are not entered;
    /// they are their own hit roots.
    fn hit_chain(&self, root: NodeId, point: Point) -> Option<Vec<NodeId>> {
        if !self.is_hit(root, point) {
            return None;
        }
        let mut path = vec![root];
        let mut current = root;
        loop {
            let children = self.nodes[current.index()].children.clone();
            let Some(next) = children
                .into_iter()
                .rev()
                .find(|child| !self.nodes[child.index()].overlay && self.is_hit(*child, point))
            else {
                break;
            };
            path.push(next);
            current = next;
        }
        Some(path)
    }

    /// False when this overlay or an ancestor is hidden. Overflow on an ancestor does not count:
    /// that is the clip an overlay escapes.
    fn layer_shown(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = &self.nodes[node_id.index()];
            if !node.alive || node.suppressed || node.style.layout.display == Display::None {
                return false;
            }
            current = node.parent;
        }
        true
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
        let mut child = id;
        let mut current = self.nodes[id.index()].parent;
        while let Some(parent) = current {
            if self.nodes[child.index()].overlay {
                return true;
            }
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
            child = parent;
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
            if self.node_disabled(id) {
                continue;
            }
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
        // Record the press before focus callbacks. A menu that closes from blur or dismiss must
        // not drop this target before release.
        let dismiss = self.outside_dismiss_targets(&self.hovered);
        self.pending_dismiss = dismiss;
        let mut click_target = None;
        for id in self.hovered.iter().rev().copied() {
            if !self.is_current(id) || self.nodes[id.index()].on_click.is_none() {
                continue;
            }
            if !self.node_disabled(id) {
                click_target = Some(id);
            }
            break;
        }
        if let Some(id) = click_target {
            self.pressed = Some(id);
            self.set_interaction(id, |state| state.pressed = true);
        }
        match self.press_focus() {
            PressFocus::Target(id) => {
                let count = self.press_count(id);
                self.pointer_capture = Some(id);
                self.focus_deferring_blur(id);
                self.pointer_down(id, count);
            }
            PressFocus::Blocked => {}
            PressFocus::None => self.blur_deferring(),
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
            self.emit_pointer(id, PointerPhase::Up, 1, PointerButton::Primary);
        }
        if let Some(id) = self.pressed.take()
            && self.is_current(id)
        {
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
        self.finish_press();
    }

    /// Deepest focusable node under the pointer. A disabled node blocks the nodes behind it.
    fn press_focus(&self) -> PressFocus {
        for id in self.hovered.iter().rev().copied() {
            if !self.is_current(id) || !self.nodes[id.index()].focusable {
                continue;
            }
            if self.node_disabled(id) {
                return PressFocus::Blocked;
            }
            return PressFocus::Target(id);
        }
        PressFocus::None
    }

    /// `on_dismiss` nodes this press did not land inside. Nested overlays count as inside their host.
    fn outside_dismiss_targets(&self, path: &[NodeId]) -> Vec<NodeId> {
        let mut targets = Vec::new();
        for index in 0..self.nodes.len() {
            if self.nodes[index].on_dismiss.is_none() {
                continue;
            }
            let id = NodeId::new(index as u32, self.nodes[index].generation);
            if !self.layer_shown(id) {
                continue;
            }
            if path.iter().any(|hit| self.is_within(*hit, id)) {
                continue;
            }
            targets.push(id);
        }
        targets
    }

    fn finish_press(&mut self) {
        self.finish_deferred_blur();
        let dismiss = std::mem::take(&mut self.pending_dismiss);
        let position = self.pointer.unwrap_or_default();
        for id in dismiss.into_iter().rev() {
            self.call_dismiss(id, position);
        }
    }

    fn finish_deferred_blur(&mut self) {
        let Some(id) = self.pending_blur.take() else {
            return;
        };
        if self.is_current(id) && self.focused != Some(id) {
            self.call_focus(id, false);
        }
    }

    fn call_dismiss(&mut self, id: NodeId, position: Point) {
        if !self.is_current(id) || !self.layer_shown(id) {
            return;
        }
        let event = ClickEvent { position };
        let mut handler = self.nodes[id.index()].on_dismiss.take();
        if let Some(handler) = handler.as_mut() {
            handler(&event);
        }
        if self.is_current(id) {
            self.nodes[id.index()].on_dismiss = handler;
        }
    }

    /// Right and middle buttons reach `on_pointer` without focusing, clicking, or dismissing.
    fn other_button(&mut self, button: PointerButton, state: ButtonState) {
        match state {
            ButtonState::Pressed => {
                let PressFocus::Target(id) = self.press_focus() else {
                    return;
                };
                self.button_capture = Some((button, id));
                self.emit_pointer(id, PointerPhase::Down, 1, button);
            }
            ButtonState::Released => {
                let Some((held, id)) = self.button_capture.take() else {
                    return;
                };
                if held != button {
                    self.button_capture = Some((held, id));
                    return;
                }
                if self.is_current(id) {
                    self.emit_pointer(id, PointerPhase::Up, 1, button);
                }
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
            self.drop_input_goal(id);
            self.reveal_input_caret(id);
            self.show_caret(id);
        }
        self.emit_pointer(id, PointerPhase::Down, count, PointerButton::Primary);
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
            self.drop_input_goal(id);
            self.reveal_input_caret(id);
            self.show_caret(id);
        }
        self.emit_pointer(id, phase, 1, PointerButton::Primary);
    }

    fn emit_pointer(
        &mut self,
        id: NodeId,
        phase: PointerPhase,
        click_count: u8,
        button: PointerButton,
    ) {
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
                button,
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
        if event.state == ButtonState::Pressed && self.move_roving(event.key) {
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
            } else if self.insert_input(id, text) == EditChange::Value {
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
                    let change = self.insert_input(id, text);
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
            Key::Home if self.is_multiline(id) => {
                self.move_input_line_edge(id, false, shift);
                return;
            }
            Key::End if self.is_multiline(id) => {
                self.move_input_line_edge(id, true, shift);
                return;
            }
            Key::Home => self.edit_input(id, |edit| edit.move_home(shift)),
            Key::End => self.edit_input(id, |edit| edit.move_end(shift)),
            Key::Up if self.is_multiline(id) => {
                self.move_input_vertically(id, false, shift);
                return;
            }
            Key::Down if self.is_multiline(id) => {
                self.move_input_vertically(id, true, shift);
                return;
            }
            Key::Enter => {
                if self.is_multiline(id) {
                    let change = self.insert_input(id, "\n");
                    if change == EditChange::Value {
                        self.publish_input(id);
                    }
                    if change != EditChange::None {
                        self.reveal_input_caret(id);
                        self.show_caret(id);
                    }
                } else {
                    self.submit_input(id);
                }
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
            self.drop_input_goal(id);
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

    /// Tab order. A visible trap replaces the page. A roving scope contributes its active item.
    fn focus_order(&self) -> Vec<NodeId> {
        let mut order = Vec::new();
        self.collect_tab_stops(self.tab_root(), &mut order);
        order
    }

    fn tab_root(&self) -> NodeId {
        self.last_visible_trap(self.root).unwrap_or(self.root)
    }

    fn last_visible_trap(&self, id: NodeId) -> Option<NodeId> {
        if !self.is_shown(id) {
            return None;
        }
        let mut found = if self.nodes[id.index()].focus_scope == Some(FocusScope::Trap) {
            Some(id)
        } else {
            None
        };
        let count = self.nodes[id.index()].children.len();
        for index in 0..count {
            let child = self.nodes[id.index()].children[index];
            if let Some(inner) = self.last_visible_trap(child) {
                found = Some(inner);
            }
        }
        found
    }

    fn collect_tab_stops(&self, id: NodeId, stops: &mut Vec<NodeId>) {
        if !self.is_shown(id) {
            return;
        }
        if self.is_roving_scope(id) {
            if self.is_focusable(id) {
                stops.push(id);
            }
            if let Some(stop) = self.roving_tab_stop(id) {
                stops.push(stop);
            }
            return;
        }
        if self.is_focusable(id) {
            stops.push(id);
        }
        let count = self.nodes[id.index()].children.len();
        for index in 0..count {
            let child = self.nodes[id.index()].children[index];
            self.collect_tab_stops(child, stops);
        }
    }

    fn roving_tab_stop(&self, scope: NodeId) -> Option<NodeId> {
        let items = self.roving_items(scope);
        if let Some(focused) = self.focused {
            if items.contains(&focused) {
                return Some(focused);
            }
        }
        if let Some(active) = self.nodes[scope.index()].roving_active {
            if self.is_current(active) && items.contains(&active) {
                return Some(active);
            }
        }
        items.first().copied()
    }

    fn roving_items(&self, scope: NodeId) -> Vec<NodeId> {
        let mut items = Vec::new();
        if !self.is_current(scope) {
            return items;
        }
        let count = self.nodes[scope.index()].children.len();
        for index in 0..count {
            let child = self.nodes[scope.index()].children[index];
            self.collect_roving_items(child, &mut items);
        }
        items
    }

    fn collect_roving_items(&self, id: NodeId, items: &mut Vec<NodeId>) {
        if !self.is_shown(id) {
            return;
        }
        if self.is_roving_scope(id) {
            if let Some(stop) = self.roving_tab_stop(id) {
                items.push(stop);
            }
            return;
        }
        if self.is_focusable(id) {
            items.push(id);
        }
        let count = self.nodes[id.index()].children.len();
        for index in 0..count {
            let child = self.nodes[id.index()].children[index];
            self.collect_roving_items(child, items);
        }
    }

    /// Moves focus inside the roving scope that contains it. False when this key is not one of
    /// that scope's arrows, so the focused node still receives it.
    fn move_roving(&mut self, key: Key) -> bool {
        let (kind, reverse) = match key {
            Key::Up => (FocusScope::Vertical, true),
            Key::Down => (FocusScope::Vertical, false),
            Key::Left => (FocusScope::Horizontal, true),
            Key::Right => (FocusScope::Horizontal, false),
            _ => return false,
        };
        let Some(focused) = self.focused else {
            return false;
        };
        if matches!(key, Key::Left | Key::Right) && self.is_input(focused) {
            return false;
        }
        if matches!(key, Key::Up | Key::Down) && self.is_multiline(focused) {
            return false;
        }
        let Some(scope) = self.scope_from(focused, kind) else {
            return false;
        };
        let items = self.roving_items(scope);
        if items.is_empty() {
            return true;
        }
        let next = match items.iter().position(|item| *item == focused) {
            Some(index) if reverse => {
                if index == 0 {
                    items.len() - 1
                } else {
                    index - 1
                }
            }
            Some(index) => (index + 1) % items.len(),
            None if reverse => items.len() - 1,
            None => 0,
        };
        self.focus(items[next]);
        true
    }

    fn scope_from(&self, id: NodeId, kind: FocusScope) -> Option<NodeId> {
        let mut current = Some(id);
        while let Some(node) = current {
            if self.is_current(node) && self.nodes[node.index()].focus_scope == Some(kind) {
                return Some(node);
            }
            current = self.nodes[node.index()].parent;
        }
        None
    }

    fn is_roving_scope(&self, id: NodeId) -> bool {
        self.is_current(id)
            && matches!(
                self.nodes[id.index()].focus_scope,
                Some(FocusScope::Vertical | FocusScope::Horizontal)
            )
    }

    /// The node and its ancestors are mounted and not hidden.
    fn is_shown(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node) = current {
            if !self.is_current(node) {
                return false;
            }
            let item = &self.nodes[node.index()];
            if item.suppressed || item.style.layout.display == Display::None {
                return false;
            }
            current = item.parent;
        }
        true
    }

    pub(super) fn arm_mounted_scope(&mut self, id: NodeId) {
        if self.is_roving_scope(id) {
            self.arm_roving(id);
        }
    }

    fn arm_roving(&mut self, id: NodeId) {
        if !self.roving_enter.contains(&id) {
            self.roving_enter.push(id);
        }
    }

    pub(super) fn roving_visibility(&mut self, id: NodeId, shown: bool) {
        if shown {
            self.arm_mounted_scope(id);
            return;
        }
        self.roving_enter.retain(|pending| *pending != id);
        if self.is_roving_scope(id) {
            self.restore_roving(id);
        }
    }

    pub(super) fn scope_display_changed(&mut self, id: NodeId, hidden: bool) {
        if hidden {
            self.hide_roving_under(id);
        } else {
            self.arm_roving_under(id);
        }
    }

    fn hide_roving_under(&mut self, id: NodeId) {
        if self.roving_enter.is_empty() && self.focused.is_none() {
            return;
        }
        if !self.is_current(id) {
            return;
        }
        self.roving_enter.retain(|pending| *pending != id);
        if self.is_roving_scope(id) {
            self.restore_roving(id);
        }
        let count = self.nodes[id.index()].children.len();
        for index in 0..count {
            let child = self.nodes[id.index()].children[index];
            self.hide_roving_under(child);
        }
    }

    fn arm_roving_under(&mut self, id: NodeId) {
        if !self.is_current(id) {
            return;
        }
        self.arm_mounted_scope(id);
        let count = self.nodes[id.index()].children.len();
        for index in 0..count {
            let child = self.nodes[id.index()].children[index];
            self.arm_roving_under(child);
        }
    }

    pub(super) fn prepare_scope_dispose(&mut self, id: NodeId) {
        self.roving_enter.retain(|pending| *pending != id);
        if self.is_roving_scope(id) {
            self.restore_roving(id);
        }
    }

    pub(super) fn settle_roving_enter(&mut self) {
        if self.roving_enter.is_empty() {
            return;
        }
        self.drain_roving_enter();
        if self.style_dirty {
            self.resolve_styles();
        }
    }

    fn drain_roving_enter(&mut self) {
        let pending = std::mem::take(&mut self.roving_enter);
        let Some(origin) = self.focused else {
            return;
        };
        for id in pending {
            if !self.is_roving_scope(id) || !self.is_shown(id) {
                continue;
            }
            if origin == id || self.is_within(origin, id) {
                continue;
            }
            if self
                .focused
                .is_some_and(|focused| focused == id || self.is_within(focused, id))
            {
                continue;
            }
            let Some(target) = self.roving_tab_stop(id) else {
                continue;
            };
            self.pin_return(id, origin);
            self.focus(target);
        }
    }

    fn pin_return(&mut self, scope: NodeId, origin: NodeId) {
        let mut current = Some(scope);
        while let Some(node) = current {
            if self.is_roving_scope(node)
                && self.nodes[node.index()].focus_return.is_none()
                && origin != node
                && !self.is_within(origin, node)
            {
                self.nodes[node.index()].focus_return = Some(origin);
            }
            current = self
                .is_current(node)
                .then_some(self.nodes[node.index()].parent)
                .flatten();
        }
    }

    fn restore_roving(&mut self, scope: NodeId) {
        if !self.is_roving_scope(scope) {
            return;
        }
        let inside = self
            .focused
            .is_some_and(|id| id == scope || self.is_within(id, scope));
        if !inside {
            return;
        }
        let return_to = self.nodes[scope.index()].focus_return.take();
        if let Some(target) = return_to {
            if target != scope
                && !self.is_within(target, scope)
                && self.is_focusable(target)
                && self.is_shown(target)
            {
                self.focus(target);
                return;
            }
        }
        self.blur();
    }

    fn remember_roving_entry(&mut self, previous: Option<NodeId>, next: NodeId) {
        if !self.is_current(next) {
            return;
        }
        let mut current = Some(next);
        while let Some(node) = current {
            if self.is_roving_scope(node) {
                if let Some(prev) = previous
                    && prev != node
                    && self.is_current(prev)
                    && !self.is_within(prev, node)
                    && self.nodes[node.index()].focus_return.is_none()
                {
                    self.nodes[node.index()].focus_return = Some(prev);
                }
                if next != node && self.is_within(next, node) {
                    self.nodes[node.index()].roving_active = Some(next);
                }
            }
            current = self.nodes[node.index()].parent;
        }
    }

    pub(super) fn focus(&mut self, id: NodeId) {
        self.focus_at(id, true);
    }

    /// Moves focus. When `notify_blur` is false, `on_focus(false)` waits until the press releases.
    fn focus_at(&mut self, id: NodeId, notify_blur: bool) {
        if self.focused == Some(id) || !self.is_focusable(id) {
            return;
        }
        let previous = self.focused;
        self.remember_roving_entry(previous, id);
        self.blur_at(notify_blur);
        self.focused = Some(id);
        self.set_interaction(id, |state| state.focused = true);
        self.call_focus(id, true);
        if self.is_input(id) {
            self.reveal_input_caret(id);
        }
        self.show_caret(id);
    }

    fn focus_deferring_blur(&mut self, id: NodeId) {
        self.focus_at(id, false);
    }

    pub(super) fn blur(&mut self) {
        self.blur_at(true);
    }

    fn blur_deferring(&mut self) {
        self.blur_at(false);
    }

    fn blur_at(&mut self, notify: bool) {
        let Some(id) = self.focused.take() else {
            return;
        };
        self.caret_deadline = None;
        if self.is_current(id) {
            self.set_interaction(id, |state| state.focused = false);
            self.mark_paint_dirty(id);
            if notify {
                self.call_focus(id, false);
            } else {
                self.pending_blur = Some(id);
            }
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
        if self.insert_input(id, &text) == EditChange::Value {
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
        let multiline = self.is_multiline(id);
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            if multiline {
                field.edit.set_lines(value.clone(), focused);
            } else {
                field.edit.set_text(value.clone(), focused);
            }
            field.published = field.edit.text().to_owned();
        }
        self.reveal_input_caret(id);
        self.mark_paint_dirty(id);
    }

    pub(super) fn apply_node_disabled(&mut self, id: NodeId, disabled: bool) {
        if !self.is_current(id) {
            return;
        }
        let unchanged = self.nodes[id.index()].disabled == disabled
            && match &self.nodes[id.index()].content {
                NodeContent::Input(field) => field.disabled == disabled,
                _ => true,
            };
        if unchanged {
            return;
        }
        let was_hovered = self.nodes[id.index()].interaction.hovered;
        let under_pointer = self.hovered.contains(&id);
        self.nodes[id.index()].disabled = disabled;
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            field.disabled = disabled;
        }
        self.set_interaction(id, |state| {
            state.disabled = disabled;
            if disabled {
                state.hovered = false;
                state.pressed = false;
            } else if under_pointer {
                state.hovered = true;
            }
        });
        if disabled && was_hovered {
            self.call_hover_handler(id, false);
        }
        if !disabled && under_pointer && !was_hovered {
            self.call_hover_handler(id, true);
        }
        if disabled && self.pressed == Some(id) {
            self.pressed = None;
        }
        if disabled && self.focused == Some(id) {
            self.blur();
        }
        self.mark_style_dirty(id);
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
        let (display, scroll, origin, text_style, multiline, mask) = {
            let node = &self.nodes[id.index()];
            let NodeContent::Input(field) = &node.content else {
                return 0;
            };
            let display = field.edit.paint(field.mask);
            if display.text.is_empty() {
                return 0;
            }
            (
                display,
                Point::new(field.scroll_x, field.scroll_y),
                content_box(node),
                node.style.text.clone(),
                field.multiline,
                field.mask,
            )
        };
        let paint_index = self.measurer.hit_test(
            &text_request(
                &display.text,
                text_style.font_size,
                text_style.line_height_px(),
                text_style.weight.0,
            ),
            multiline.then_some(origin.width),
            text_style.align,
            origin.width,
            local.x - origin.x + scroll.x,
            local.y - origin.y + scroll.y,
        );
        let NodeContent::Input(field) = &self.nodes[id.index()].content else {
            return 0;
        };
        field.edit.index_from_paint(paint_index, mask)
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
        let shown = if text_style.ellipsis {
            super::passes::ellipsize_text(
                self.measurer.as_mut(),
                &text,
                &text_style,
                origin_rect.width,
            )
        } else {
            text
        };
        let width = origin_rect.width;
        self.measurer.hit_test(
            &text_request(
                &shown,
                text_style.font_size,
                text_style.line_height_px(),
                text_style.weight.0,
            ),
            (!text_style.ellipsis).then_some(width),
            text_style.align,
            width,
            local.x - origin_rect.x,
            local.y - origin_rect.y,
        )
    }

    fn reveal_input_caret(&mut self, id: NodeId) {
        if !self.is_input(id) {
            return;
        }
        self.sync_multiline_sample(id);
        let (display, scroll, view, text_style, multiline) = {
            let node = &self.nodes[id.index()];
            let NodeContent::Input(field) = &node.content else {
                return;
            };
            let display = field.edit.paint(field.mask);
            (
                display,
                Point::new(field.scroll_x, field.scroll_y),
                content_box(node),
                node.style.text.clone(),
                field.multiline,
            )
        };
        if view.width <= 0.0 || view.height <= 0.0 {
            return;
        }
        let caret = display.caret.min(display.text.len());
        let geometry = self.measurer.caret_geometry(
            &text_request(
                &display.text,
                text_style.font_size,
                text_style.line_height_px(),
                text_style.weight.0,
            ),
            multiline.then_some(view.width),
            text_style.align,
            view.width,
            caret,
        );
        let measured = self.measurer.measure(
            &text_request(
                &display.text,
                text_style.font_size,
                text_style.line_height_px(),
                text_style.weight.0,
            ),
            multiline.then_some(view.width),
        );
        let next_x = reveal_offset(geometry.x, scroll.x, view.width)
            .min((measured.width - view.width).max(0.0));
        let next_y = if multiline {
            reveal_span(geometry.y, geometry.line_height, scroll.y, view.height)
                .min((measured.height - view.height).max(0.0))
        } else {
            0.0
        };
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content
            && ((field.scroll_x - next_x).abs() > f32::EPSILON
                || (field.scroll_y - next_y).abs() > f32::EPSILON)
        {
            field.scroll_x = next_x;
            field.scroll_y = next_y;
            self.mark_paint_dirty(id);
        }
    }

    fn insert_input(&mut self, id: NodeId, text: &str) -> EditChange {
        let multiline = self.is_multiline(id);
        self.edit_input(id, |edit| {
            if multiline {
                edit.insert_lines(text)
            } else {
                edit.insert(text)
            }
        })
    }

    fn is_multiline(&self, id: NodeId) -> bool {
        matches!(
            &self.nodes[id.index()].content,
            NodeContent::Input(field) if field.multiline
        )
    }

    fn drop_input_goal(&mut self, id: NodeId) {
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            field.goal_x = None;
        }
    }

    fn move_input_vertically(&mut self, id: NodeId, down: bool, extend: bool) {
        let Some(probe) = self.input_probe(id) else {
            return;
        };
        let geometry = self.caret_at(&probe, probe.caret);
        let goal_x = probe.goal_x.unwrap_or(geometry.x);
        let y = if down {
            geometry.y + geometry.line_height + 0.5
        } else {
            geometry.y - 0.5
        };
        let paint_index = self.measurer.hit_test(
            &text_request(
                &probe.text,
                probe.font_size,
                probe.line_height,
                probe.weight,
            ),
            Some(probe.width),
            probe.align,
            probe.width,
            goal_x,
            y,
        );
        self.place_painted(id, paint_index, probe.mask, extend);
        if let NodeContent::Input(field) = &mut self.nodes[id.index()].content {
            field.goal_x = Some(goal_x);
        }
        self.reveal_input_caret(id);
        self.show_caret(id);
    }

    fn move_input_line_edge(&mut self, id: NodeId, end: bool, extend: bool) {
        let Some(probe) = self.input_probe(id) else {
            return;
        };
        let geometry = self.caret_at(&probe, probe.caret);
        let x = if end { probe.width + 1000.0 } else { -1000.0 };
        let paint_index = self.measurer.hit_test(
            &text_request(
                &probe.text,
                probe.font_size,
                probe.line_height,
                probe.weight,
            ),
            Some(probe.width),
            probe.align,
            probe.width,
            x,
            geometry.y + geometry.line_height * 0.5,
        );
        self.drop_input_goal(id);
        self.place_painted(id, paint_index, probe.mask, extend);
        self.reveal_input_caret(id);
        self.show_caret(id);
    }

    fn place_painted(&mut self, id: NodeId, paint_index: usize, mask: bool, extend: bool) {
        let real = match &self.nodes[id.index()].content {
            NodeContent::Input(field) => field.edit.index_from_paint(paint_index, mask),
            NodeContent::Container | NodeContent::Text(_) | NodeContent::Image { .. } => return,
        };
        self.edit_input(id, |edit| edit.place(real, extend));
    }

    fn caret_at(&mut self, probe: &InputProbe, index: usize) -> cappy_layout::CaretGeometry {
        self.measurer.caret_geometry(
            &text_request(
                &probe.text,
                probe.font_size,
                probe.line_height,
                probe.weight,
            ),
            probe.multiline.then_some(probe.width),
            probe.align,
            probe.width,
            index,
        )
    }

    fn input_probe(&self, id: NodeId) -> Option<InputProbe> {
        let node = &self.nodes[id.index()];
        let NodeContent::Input(field) = &node.content else {
            return None;
        };
        let display = field.edit.paint(field.mask);
        let origin = content_box(node);
        let style = &node.style.text;
        let caret = display.caret.min(display.text.len());
        Some(InputProbe {
            text: display.text,
            caret,
            mask: field.mask,
            multiline: field.multiline,
            goal_x: field.goal_x,
            width: origin.width,
            align: style.align,
            font_size: style.font_size,
            line_height: style.line_height_px(),
            weight: style.weight.0,
        })
    }

    pub(super) fn cached_ime_caret(&mut self) -> Option<Rect> {
        let Some(id) = self.focused else {
            self.ime_caret = None;
            return None;
        };
        if !self.is_current(id) || !self.is_input(id) {
            self.ime_caret = None;
            return None;
        }
        let node = &self.nodes[id.index()];
        let NodeContent::Input(field) = &node.content else {
            self.ime_caret = None;
            return None;
        };
        if node.rect.width <= 0.0 && node.rect.height <= 0.0 {
            self.ime_caret = None;
            return None;
        }
        let display = field.edit.paint(field.mask);
        let origin = content_box(node);
        let style = &node.style.text;
        let key = ImeCaretKey {
            text: display.text.clone(),
            caret: display.caret.min(display.text.len()),
            scroll_x: field.scroll_x.to_bits(),
            scroll_y: field.scroll_y.to_bits(),
            origin_x: origin.x.to_bits(),
            origin_y: origin.y.to_bits(),
            origin_w: origin.width.to_bits(),
            origin_h: origin.height.to_bits(),
            align: align_tag(style.align),
            font_size: style.font_size.to_bits(),
            line_height: style.line_height_px().to_bits(),
            weight: style.weight.0,
            multiline: field.multiline,
        };
        if let Some(cache) = &self.ime_caret
            && cache.key == key
        {
            return Some(cache.rect);
        }
        let scroll = Point::new(field.scroll_x, field.scroll_y);
        let multiline = field.multiline;
        let align = style.align;
        let font_size = style.font_size;
        let line_height = style.line_height_px();
        let weight = style.weight.0;
        let origin_x = origin.x;
        let origin_y = origin.y;
        let origin_width = origin.width;
        let geometry = self.measurer.caret_geometry(
            &text_request(&key.text, font_size, line_height, weight),
            multiline.then_some(origin_width),
            align,
            origin_width,
            key.caret,
        );
        let rect = Rect::new(
            origin_x + geometry.x - scroll.x,
            origin_y + geometry.y - scroll.y,
            1.0,
            geometry.line_height,
        );
        self.ime_caret = Some(ImeCaretCache { key, rect });
        Some(rect)
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

struct InputProbe {
    text: String,
    caret: usize,
    mask: bool,
    multiline: bool,
    goal_x: Option<f32>,
    width: f32,
    align: TextAlign,
    font_size: f32,
    line_height: f32,
    weight: u16,
}

pub(super) struct ImeCaretCache {
    key: ImeCaretKey,
    rect: Rect,
}

#[derive(PartialEq)]
struct ImeCaretKey {
    text: String,
    caret: usize,
    scroll_x: u32,
    scroll_y: u32,
    origin_x: u32,
    origin_y: u32,
    origin_w: u32,
    origin_h: u32,
    align: u8,
    font_size: u32,
    line_height: u32,
    weight: u16,
    multiline: bool,
}

fn align_tag(align: TextAlign) -> u8 {
    match align {
        TextAlign::Start => 0,
        TextAlign::Center => 1,
        TextAlign::End => 2,
    }
}

fn text_request(
    text: &str,
    font_size: f32,
    line_height: f32,
    weight: u16,
) -> TextMeasureRequest<'_> {
    TextMeasureRequest {
        text,
        font_size,
        line_height,
        weight,
    }
}

/// Scroll that keeps a caret point in view. A point already inside the first view stays at zero,
/// so a measurement made before the box had a size does not stick.
fn reveal_offset(position: f32, scroll: f32, view: f32) -> f32 {
    if view <= 0.0 || position <= view {
        return 0.0;
    }
    if position < scroll {
        position.max(0.0)
    } else if position > scroll + view {
        (position - view).max(0.0)
    } else {
        scroll
    }
}

/// Scroll that keeps one line in view. A line that fits from the origin stays at zero.
fn reveal_span(top: f32, height: f32, scroll: f32, view: f32) -> f32 {
    let bottom = top + height;
    if view <= 0.0 || bottom <= view {
        return 0.0;
    }
    if top < scroll {
        top.max(0.0)
    } else if bottom > scroll + view {
        (bottom - view).max(0.0)
    } else {
        scroll
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
