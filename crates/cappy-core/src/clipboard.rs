//! Text clipboard. The window host installs the platform clipboard; tests use the in-memory one.
//! The browser build installs a clipboard that does nothing, because the web clipboard API is
//! asynchronous and needs a user gesture the host does not have here.

/// Reads and writes plain text. A failed platform call returns [`None`] or keeps the previous
/// text; it does not panic.
pub trait Clipboard {
    fn read_text(&mut self) -> Option<String>;
    fn write_text(&mut self, text: &str);
}

/// Clipboard that lives in the process. [`Ui`](crate::Ui) starts with one, so tests can copy and
/// paste without a window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryClipboard {
    text: Option<String>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for MemoryClipboard {
    fn read_text(&mut self) -> Option<String> {
        self.text.clone()
    }

    fn write_text(&mut self, text: &str) {
        self.text = Some(text.to_owned());
    }
}

/// Clipboard that never stores text. The web host installs this.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopClipboard;

impl Clipboard for NoopClipboard {
    fn read_text(&mut self) -> Option<String> {
        None
    }

    fn write_text(&mut self, _text: &str) {}
}
