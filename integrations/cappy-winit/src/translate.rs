//! winit types to cappy types.

use cappy_core::{ButtonState, CursorIcon, InputEvent, Point, PointerButton};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

/// Logical pixels one wheel line moves the content.
///
/// On Windows a notch is one line (`delta / 120`). 100px is Chrome's default for that notch,
/// about three text rows. Pixel deltas from a trackpad are not scaled.
const WHEEL_LINE_PIXELS: f32 = 100.0;

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

/// `delta` is content movement in logical pixels. Positive moves content down and right.
pub fn mouse_wheel(delta: MouseScrollDelta, scale_factor: f64) -> InputEvent {
    let (x, y) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (x * WHEEL_LINE_PIXELS, y * WHEEL_LINE_PIXELS),
        MouseScrollDelta::PixelDelta(position) => {
            let logical = position.to_logical::<f32>(scale_factor);
            (logical.x, logical.y)
        }
    };
    InputEvent::Wheel {
        delta: Point::new(x, y),
    }
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

    #[test]
    fn a_wheel_notch_moves_content_by_one_hundred_logical_pixels() {
        assert_eq!(
            mouse_wheel(MouseScrollDelta::LineDelta(0.0, 1.0), 2.0),
            InputEvent::Wheel {
                delta: Point::new(0.0, 100.0)
            }
        );
        assert_eq!(
            mouse_wheel(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(20.0, -10.0)),
                2.0
            ),
            InputEvent::Wheel {
                delta: Point::new(10.0, -5.0)
            }
        );
    }
}
