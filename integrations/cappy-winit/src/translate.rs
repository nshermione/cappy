//! winit types to cappy types.

use cappy_core::{ButtonState, CursorIcon, InputEvent, Point, PointerButton};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};

pub fn pointer_moved(position: PhysicalPosition<f64>, scale_factor: f64) -> InputEvent {
    let logical = position.to_logical::<f32>(scale_factor);
    InputEvent::PointerMoved {
        position: Point::new(logical.x, logical.y),
    }
}

/// `None` for buttons cappy does not handle (back, forward, ...).
pub fn pointer_button(button: MouseButton, state: ElementState) -> Option<InputEvent> {
    let button = match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => return None,
    };
    let state = match state {
        ElementState::Pressed => ButtonState::Pressed,
        ElementState::Released => ButtonState::Released,
    };
    Some(InputEvent::PointerButton { button, state })
}

pub fn cursor_icon(cursor: CursorIcon) -> winit::window::CursorIcon {
    match cursor {
        CursorIcon::Default => winit::window::CursorIcon::Default,
        CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_become_logical() {
        let event = pointer_moved(PhysicalPosition::new(200.0, 100.0), 2.0);
        assert_eq!(
            event,
            InputEvent::PointerMoved {
                position: Point::new(100.0, 50.0)
            }
        );
    }

    #[test]
    fn left_button_is_primary_and_extra_buttons_are_ignored() {
        assert_eq!(
            pointer_button(MouseButton::Left, ElementState::Pressed),
            Some(InputEvent::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            })
        );
        assert_eq!(
            pointer_button(MouseButton::Back, ElementState::Pressed),
            None
        );
    }
}
