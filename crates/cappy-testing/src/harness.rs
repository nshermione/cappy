//! Headless driver for writing UI tests against a [`Ui`] without a window or GPU.

use cappy_core::{
    ButtonState, Damage, Element, FrameInput, InputEvent, NodeId, Point, PointerButton, Rect,
    RenderFrame, Theme, Ui, UpdateOutcome,
};

const DEFAULT_VIEWPORT: FrameInput = FrameInput {
    physical_width: 800,
    physical_height: 600,
    scale_factor: 1.0,
};

/// Drives a [`Ui`] like a host would, addressing nodes by their `key`.
///
/// Lookups panic with a descriptive message, which is what a failing test should do.
pub struct TestUi {
    ui: Ui,
    viewport: FrameInput,
}

impl TestUi {
    /// Mounts `root` in an 800x600 viewport and runs the first update.
    pub fn new(root: impl Into<Element>, theme: Theme) -> Self {
        Self::from_ui(Ui::new(root, theme))
    }

    /// Wraps an already configured [`Ui`] and runs the first update.
    pub fn from_ui(ui: Ui) -> Self {
        let mut harness = Self {
            ui,
            viewport: DEFAULT_VIEWPORT,
        };
        harness.frame();
        harness
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    pub fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
    }

    pub fn viewport(&self) -> FrameInput {
        self.viewport
    }

    /// Changes the viewport; takes effect on the next update.
    pub fn resize(&mut self, viewport: FrameInput) {
        self.viewport = viewport;
    }

    /// Runs an update without consuming its damage.
    pub fn update(&mut self) -> UpdateOutcome {
        self.ui.update(self.viewport)
    }

    /// Runs an update and takes the resulting frame, as a host does before rendering.
    pub fn frame(&mut self) -> RenderFrame {
        self.update();
        self.ui
            .prepare_frame()
            .expect("a frame is always available after an update")
    }

    /// Damage produced by everything that happened since the last frame.
    pub fn damage(&mut self) -> Damage {
        self.frame().damage
    }

    pub fn node(&self, key: &str) -> NodeId {
        self.ui
            .find_by_key(key)
            .unwrap_or_else(|| panic!("no node with key {key:?}"))
    }

    pub fn rect(&self, key: &str) -> Rect {
        self.ui
            .node_rect(self.node(key))
            .unwrap_or_else(|| panic!("node {key:?} has no layout yet; call update first"))
    }

    pub fn center(&self, key: &str) -> Point {
        let rect = self.rect(key);
        Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    /// Text of the node with `key`, or of its first text child.
    pub fn text(&self, key: &str) -> &str {
        let node = self.node(key);
        let own_text = self.ui.node_text(node);
        own_text
            .or_else(|| {
                self.ui
                    .node_children(node)
                    .iter()
                    .find_map(|&child| self.ui.node_text(child))
            })
            .unwrap_or_else(|| panic!("node {key:?} has no text"))
    }

    pub fn move_to(&mut self, key: &str) {
        let position = self.center(key);
        self.move_pointer(position);
    }

    pub fn move_pointer(&mut self, position: Point) {
        self.ui.handle_event(InputEvent::PointerMoved { position });
    }

    pub fn press(&mut self) {
        self.primary_button(ButtonState::Pressed);
    }

    pub fn release(&mut self) {
        self.primary_button(ButtonState::Released);
    }

    /// Moves to the center of `key`, then presses and releases the primary button.
    pub fn click(&mut self, key: &str) {
        self.move_to(key);
        self.press();
        self.release();
    }

    pub fn leave(&mut self) {
        self.ui.handle_event(InputEvent::PointerLeft);
    }

    fn primary_button(&mut self, state: ButtonState) {
        self.ui.handle_event(InputEvent::PointerButton {
            button: PointerButton::Primary,
            state,
        });
    }
}
