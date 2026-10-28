//! Platform-neutral input events. Window-manager integrations translate their native events
//! into these, with positions in logical pixels.

use cappy_layout::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    PointerMoved {
        position: Point,
    },
    PointerButton {
        button: PointerButton,
        state: ButtonState,
    },
    /// The pointer left the UI surface.
    PointerLeft,
}

/// Passed to click handlers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickEvent {
    pub position: Point,
}

/// Cursor the host should show for the current pointer position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorIcon {
    #[default]
    Default,
    Pointer,
}
