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

/// Shift, Ctrl, Alt and the platform meta key (Command on macOS, Windows key elsewhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    /// The shortcut modifier: Ctrl, or Command on macOS. Both are accepted so a shortcut works
    /// on either platform without the UI asking which one it is on.
    pub const fn primary(self) -> bool {
        self.ctrl || self.meta
    }
}

/// A logical key. Character input arrives as [`InputEvent::Text`]; this identifies the key for
/// navigation and shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    /// A printable key. Shortcuts compare the ASCII lowercase form.
    Character(char),
}

/// A key press or release, including auto-repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub state: ButtonState,
    pub repeat: bool,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    pub fn shortcut(self) -> Option<char> {
        match self.key {
            Key::Character(ch) if self.modifiers.primary() => Some(ch.to_ascii_lowercase()),
            _ => None,
        }
    }
}

/// IME composition. `cursor` is a byte offset into `text` while the platform is still composing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    Preedit { text: String, cursor: Option<usize> },
    Commit(String),
}

/// Pointer phase delivered to a focusable node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
}

/// Pointer activity on a focusable node. `text_index` is a byte offset into the text leaf under
/// the pointer when that leaf can be hit.
///
/// `button` is the button for this event. Move events belong to the primary gesture. Secondary
/// and middle buttons are delivered as down and up only, and they do not focus or click.
#[derive(Clone, Debug, PartialEq)]
pub struct PointerEvent {
    pub position: Point,
    pub phase: PointerPhase,
    pub button: PointerButton,
    pub text_index: Option<usize>,
    pub shift: bool,
    pub click_count: u8,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Wheel or trackpad scroll at the current pointer position.
    ///
    /// Positive values move content right and down, revealing what was above and to the left.
    Wheel {
        delta: Point,
    },
    /// The platform modifier mask. Later key and pointer events also carry the keys they need.
    Modifiers(Modifiers),
    Key(KeyEvent),
    /// Committed text, already composed. One event may hold several characters.
    Text(String),
    Ime(ImeEvent),
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
    /// I-beam over text that can take a caret.
    Text,
}
