//! tao types to cappy types.

use cappy_core::{ButtonState, CursorIcon, InputEvent, Point, PointerButton};
use tao::dpi::PhysicalPosition;
use tao::event::{ElementState, MouseButton};

pub fn pointer_moved(position: PhysicalPosition<f64>, scale_factor: f64) -> InputEvent {
    let logical = position.to_logical::<f32>(scale_factor);
    InputEvent::PointerMoved {
        position: Point::new(logical.x, logical.y),
    }
}

/// `None` for buttons cappy does not handle, and for states tao may add later.
pub fn pointer_button(button: MouseButton, state: ElementState) -> Option<InputEvent> {
    let button = match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        _ => return None,
    };
    let state = match state {
        ElementState::Pressed => ButtonState::Pressed,
        ElementState::Released => ButtonState::Released,
        _ => return None,
    };
    Some(InputEvent::PointerButton { button, state })
}

pub fn cursor_icon(cursor: CursorIcon) -> tao::window::CursorIcon {
    match cursor {
        CursorIcon::Default => tao::window::CursorIcon::Default,
        CursorIcon::Pointer => tao::window::CursorIcon::Hand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_become_logical() {
        let event = pointer_moved(PhysicalPosition::new(300.0, 150.0), 1.5);
        assert_eq!(
            event,
            InputEvent::PointerMoved {
                position: Point::new(200.0, 100.0)
            }
        );
    }

    #[test]
    fn left_button_is_primary_and_extra_buttons_are_ignored() {
        assert_eq!(
            pointer_button(MouseButton::Left, ElementState::Released),
            Some(InputEvent::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Released,
            })
        );
        assert_eq!(
            pointer_button(MouseButton::Other(4), ElementState::Pressed),
            None
        );
    }
}
