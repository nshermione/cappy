//! Pointer handling: hit testing, hover/pressed state and click dispatch.

use cappy_layout::Point;
use cappy_style::{Display, InteractionState};

use super::{NodeId, Ui};
use crate::{ButtonState, ClickEvent, InputEvent, PointerButton};

impl Ui {
    /// Feeds one input event. Call [`Ui::update`] afterwards if [`Ui::needs_update`] is true.
    pub fn handle_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::PointerMoved { position } => {
                self.pointer = Some(position);
                self.refresh_hover();
            }
            InputEvent::PointerLeft => {
                self.pointer = None;
                self.refresh_hover();
            }
            InputEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            } => match state {
                ButtonState::Pressed => self.press(),
                ButtonState::Released => self.release(),
            },
            InputEvent::PointerButton { .. } => {}
        }
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
        let node = &self.nodes[id.index()];
        node.style.layout.display != Display::None && node.rect.contains(point)
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
        for id in previous.iter().filter(|id| !path.contains(id)) {
            self.set_interaction(*id, |state| state.hovered = false);
            self.call_hover_handler(*id, false);
        }
        for id in path.iter().filter(|id| !previous.contains(id)) {
            self.set_interaction(*id, |state| state.hovered = true);
            self.call_hover_handler(*id, true);
        }
    }

    fn press(&mut self) {
        let target = self
            .hovered
            .iter()
            .rev()
            .copied()
            .find(|id| self.nodes[id.index()].on_click.is_some());
        if let Some(id) = target {
            self.pressed = Some(id);
            self.set_interaction(id, |state| state.pressed = true);
        }
    }

    fn release(&mut self) {
        let Some(id) = self.pressed.take() else {
            return;
        };
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

    fn set_interaction(&mut self, id: NodeId, change: impl FnOnce(&mut InteractionState)) {
        let node = &mut self.nodes[id.index()];
        let before = node.interaction;
        change(&mut node.interaction);
        if node.interaction != before {
            self.mark_style_dirty(id);
        }
    }

    fn call_hover_handler(&mut self, id: NodeId, hovered: bool) {
        let node = &mut self.nodes[id.index()];
        if let Some(mut handler) = node.on_hover.take() {
            handler(hovered);
            node.on_hover.get_or_insert(handler);
        }
    }
}
