//! Single-string edits shared by the built-in input. A custom editor keeps its own buffer and
//! does not call this.

use unicode_segmentation::UnicodeSegmentation;

const UNDO_LIMIT: usize = 64;

#[derive(Clone, Debug)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: usize,
}

#[derive(Clone, Debug)]
struct Preedit {
    text: String,
    cursor: usize,
}

/// What an edit did. Value changes are written back to a bound signal; caret changes only repaint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditChange {
    None,
    Caret,
    Value,
}

/// A plain string, a caret and a selection anchor, both byte indexes on grapheme boundaries.
#[derive(Clone, Debug)]
pub(crate) struct TextEdit {
    text: String,
    caret: usize,
    anchor: usize,
    preedit: Option<Preedit>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl TextEdit {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.len();
        Self {
            caret: end,
            anchor: end,
            text,
            preedit: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the string. `keep_caret` clamps the current caret; otherwise the caret goes to
    /// the end, which is the initial value before the field is focused.
    pub(crate) fn set_text(&mut self, text: String, keep_caret: bool) {
        if keep_caret {
            self.caret = floor_grapheme(&text, self.caret);
            self.anchor = floor_grapheme(&text, self.anchor);
        } else {
            self.caret = text.len();
            self.anchor = self.caret;
        }
        self.text = text;
        self.preedit = None;
    }

    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        if self.preedit.is_some() || self.caret == self.anchor {
            None
        } else {
            Some((self.caret.min(self.anchor), self.caret.max(self.anchor)))
        }
    }

    /// Text as drawn: the value with the IME preedit spliced in at the caret.
    pub(crate) fn display(&self) -> DisplayText {
        let Some(preedit) = &self.preedit else {
            return DisplayText {
                text: self.text.clone(),
                caret: self.caret,
                selection: self.selection(),
            };
        };
        let mut text = String::new();
        text.push_str(&self.text[..self.caret]);
        let caret = text.len() + preedit.cursor;
        text.push_str(&preedit.text);
        text.push_str(&self.text[self.caret..]);
        DisplayText {
            text,
            caret,
            selection: None,
        }
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text[start..end].to_owned())
    }

    /// Drops the selection and moves the caret to `index`. `extend` keeps the anchor, which is
    /// how Shift+click and a drag grow a selection.
    pub(crate) fn place(&mut self, index: usize, extend: bool) -> EditChange {
        self.preedit = None;
        let index = floor_grapheme(&self.text, index);
        if self.caret == index && (extend || self.anchor == index) {
            return EditChange::None;
        }
        self.caret = index;
        if !extend {
            self.anchor = index;
        }
        EditChange::Caret
    }

    pub(crate) fn select_word(&mut self, index: usize) -> EditChange {
        self.preedit = None;
        let (start, end) = word_bounds(&self.text, floor_grapheme(&self.text, index));
        if self.anchor == start && self.caret == end {
            return EditChange::None;
        }
        self.anchor = start;
        self.caret = end;
        EditChange::Caret
    }

    pub(crate) fn select_all(&mut self) -> EditChange {
        self.preedit = None;
        if self.anchor == 0 && self.caret == self.text.len() {
            return EditChange::None;
        }
        self.anchor = 0;
        self.caret = self.text.len();
        EditChange::Caret
    }

    pub(crate) fn insert(&mut self, extra: &str) -> EditChange {
        let extra = single_line(extra);
        self.preedit = None;
        if extra.is_empty() && self.caret == self.anchor {
            return EditChange::None;
        }
        self.snapshot();
        self.delete_selection();
        self.text.insert_str(self.caret, &extra);
        self.caret += extra.len();
        self.anchor = self.caret;
        EditChange::Value
    }

    pub(crate) fn set_preedit(&mut self, text: &str, cursor: Option<usize>) -> EditChange {
        if text.is_empty() {
            let changed = self.preedit.take().is_some();
            return if changed {
                EditChange::Caret
            } else {
                EditChange::None
            };
        }
        let replaced = if self.preedit.is_none() && self.caret != self.anchor {
            self.snapshot();
            self.delete_selection();
            true
        } else {
            false
        };
        let cursor = floor_grapheme(text, cursor.unwrap_or(text.len()));
        self.preedit = Some(Preedit {
            text: text.to_owned(),
            cursor,
        });
        if replaced {
            EditChange::Value
        } else {
            EditChange::Caret
        }
    }

    pub(crate) fn backspace(&mut self, word: bool) -> EditChange {
        self.erase(false, word)
    }

    pub(crate) fn delete_forward(&mut self, word: bool) -> EditChange {
        self.erase(true, word)
    }

    pub(crate) fn move_left(&mut self, extend: bool, word: bool) -> EditChange {
        let index = if word {
            move_word_left(&self.text, self.caret)
        } else {
            prev_grapheme(&self.text, self.caret)
        };
        self.move_to(index, extend)
    }

    pub(crate) fn move_right(&mut self, extend: bool, word: bool) -> EditChange {
        let index = if word {
            move_word_right(&self.text, self.caret)
        } else {
            next_grapheme(&self.text, self.caret)
        };
        self.move_to(index, extend)
    }

    pub(crate) fn move_home(&mut self, extend: bool) -> EditChange {
        self.move_to(0, extend)
    }

    pub(crate) fn move_end(&mut self, extend: bool) -> EditChange {
        self.move_to(self.text.len(), extend)
    }

    pub(crate) fn cut(&mut self) -> Option<String> {
        let selected = self.selected_text()?;
        self.snapshot();
        self.delete_selection();
        Some(selected)
    }

    pub(crate) fn undo(&mut self) -> EditChange {
        self.restore(true)
    }

    pub(crate) fn redo(&mut self) -> EditChange {
        self.restore(false)
    }

    fn erase(&mut self, forward: bool, word: bool) -> EditChange {
        self.preedit = None;
        if self.caret != self.anchor {
            self.snapshot();
            self.delete_selection();
            return EditChange::Value;
        }
        let target = if forward {
            if word {
                move_word_right(&self.text, self.caret)
            } else {
                next_grapheme(&self.text, self.caret)
            }
        } else if word {
            move_word_left(&self.text, self.caret)
        } else {
            prev_grapheme(&self.text, self.caret)
        };
        if target == self.caret {
            return EditChange::None;
        }
        self.snapshot();
        let (start, end) = if target < self.caret {
            (target, self.caret)
        } else {
            (self.caret, target)
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = start;
        EditChange::Value
    }

    fn move_to(&mut self, index: usize, extend: bool) -> EditChange {
        self.preedit = None;
        let index = floor_grapheme(&self.text, index);
        if self.caret == index && (extend || self.anchor == index) {
            return EditChange::None;
        }
        self.caret = index;
        if !extend {
            self.anchor = index;
        }
        EditChange::Caret
    }

    fn delete_selection(&mut self) {
        let Some((start, end)) = self.selection_bounds() else {
            return;
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = start;
    }

    fn selection_bounds(&self) -> Option<(usize, usize)> {
        if self.caret == self.anchor {
            None
        } else {
            Some((self.caret.min(self.anchor), self.caret.max(self.anchor)))
        }
    }

    fn snapshot(&mut self) {
        self.redo.clear();
        self.undo.push(Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, undo: bool) -> EditChange {
        self.preedit = None;
        let stack = if undo { &mut self.undo } else { &mut self.redo };
        let Some(snapshot) = stack.pop() else {
            return EditChange::None;
        };
        let current = Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        };
        if undo {
            self.redo.push(current);
        } else {
            self.undo.push(current);
        }
        self.text = snapshot.text;
        self.caret = floor_grapheme(&self.text, snapshot.caret);
        self.anchor = floor_grapheme(&self.text, snapshot.anchor);
        EditChange::Value
    }
}

/// The string a field paints, plus where the caret and selection sit inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DisplayText {
    pub(crate) text: String,
    pub(crate) caret: usize,
    pub(crate) selection: Option<(usize, usize)>,
}

fn single_line(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch != '\n' && *ch != '\r')
        .collect()
}

fn floor_grapheme(text: &str, index: usize) -> usize {
    let index = index.min(text.len());
    // `len` is the gap after the last grapheme. It is not a start offset, so it has to be
    // kept on its own; otherwise a click past the text lands on the last character.
    if index == text.len() {
        return index;
    }
    text.grapheme_indices(true)
        .map(|(offset, _)| offset)
        .take_while(|offset| *offset <= index)
        .last()
        .unwrap_or(0)
}

fn prev_grapheme(text: &str, index: usize) -> usize {
    let index = floor_grapheme(text, index);
    text.grapheme_indices(true)
        .map(|(offset, _)| offset)
        .take_while(|offset| *offset < index)
        .last()
        .unwrap_or(0)
}

fn next_grapheme(text: &str, index: usize) -> usize {
    let index = floor_grapheme(text, index);
    text.grapheme_indices(true)
        .map(|(offset, _)| offset)
        .find(|offset| *offset > index)
        .unwrap_or(text.len())
}

fn skip_left(text: &str, mut index: usize, pred: impl Fn(char) -> bool) -> usize {
    while index > 0 {
        let previous = prev_grapheme(text, index);
        let ch = text[previous..index].chars().next().unwrap_or('\0');
        if !pred(ch) {
            break;
        }
        index = previous;
    }
    index
}

fn skip_right(text: &str, mut index: usize, pred: impl Fn(char) -> bool) -> usize {
    while index < text.len() {
        let next = next_grapheme(text, index);
        let ch = text[index..next].chars().next().unwrap_or('\0');
        if !pred(ch) {
            break;
        }
        index = next;
    }
    index
}

fn move_word_left(text: &str, index: usize) -> usize {
    let index = floor_grapheme(text, index);
    if index == 0 {
        return 0;
    }
    let previous = prev_grapheme(text, index);
    let ch = text[previous..index].chars().next().unwrap_or('\0');
    if ch.is_whitespace() {
        let skipped = skip_left(text, index, char::is_whitespace);
        skip_left(text, skipped, |ch| !ch.is_whitespace())
    } else {
        skip_left(text, index, |ch| !ch.is_whitespace())
    }
}

fn move_word_right(text: &str, index: usize) -> usize {
    let index = floor_grapheme(text, index);
    if index >= text.len() {
        return text.len();
    }
    let next = next_grapheme(text, index);
    let ch = text[index..next].chars().next().unwrap_or('\0');
    if ch.is_whitespace() {
        let skipped = skip_right(text, index, char::is_whitespace);
        skip_right(text, skipped, |ch| !ch.is_whitespace())
    } else {
        let word = skip_right(text, index, |ch| !ch.is_whitespace());
        skip_right(text, word, char::is_whitespace)
    }
}

fn word_bounds(text: &str, index: usize) -> (usize, usize) {
    let index = floor_grapheme(text, index).min(text.len());
    if text.is_empty() {
        return (0, 0);
    }
    let at = if index >= text.len() {
        prev_grapheme(text, text.len())
    } else {
        index
    };
    let next = next_grapheme(text, at);
    let ch = text[at..next].chars().next().unwrap_or('\0');
    if ch.is_whitespace() {
        (
            skip_left(text, next, char::is_whitespace),
            skip_right(text, at, char::is_whitespace),
        )
    } else {
        (
            skip_left(text, next, |ch| !ch.is_whitespace()),
            skip_right(text, at, |ch| !ch.is_whitespace()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str) -> TextEdit {
        let mut edit = TextEdit::new(text);
        edit.place(0, false);
        edit
    }

    #[test]
    fn place_at_the_end_stays_after_the_last_grapheme() {
        let mut edit = TextEdit::new("hếllo");
        edit.place(0, false);
        edit.place(edit.text().len(), false);
        assert_eq!(edit.display().caret, edit.text().len());
        edit.insert("Z");
        assert_eq!(edit.text(), "hếlloZ");
    }

    #[test]
    fn insert_and_paste_land_at_the_caret() {
        let mut edit = edit("hello");
        edit.place(2, false);
        assert_eq!(edit.insert("XX"), EditChange::Value);
        assert_eq!(edit.text(), "heXXllo");
        assert_eq!(edit.display().caret, 4);
    }

    #[test]
    fn paste_replaces_the_selection() {
        let mut edit = edit("hello");
        edit.place(2, false);
        edit.place(4, true);
        assert_eq!(edit.selected_text().as_deref(), Some("ll"));
        edit.insert("XX");
        assert_eq!(edit.text(), "heXXo");
        assert!(edit.selection().is_none());
    }

    #[test]
    fn select_all_and_copy_cover_the_value() {
        let mut edit = TextEdit::new("hello");
        edit.select_all();
        assert_eq!(edit.selected_text().as_deref(), Some("hello"));
        assert_eq!(edit.selection(), Some((0, 5)));
    }

    #[test]
    fn backspace_deletes_the_selection_or_the_previous_grapheme() {
        let mut edit = TextEdit::new("hếllo");
        edit.place(0, false);
        edit.place("hế".len(), false);
        edit.backspace(false);
        assert_eq!(edit.text(), "hllo");
        edit.select_all();
        edit.backspace(false);
        assert_eq!(edit.text(), "");
    }

    #[test]
    fn shift_move_extends_and_a_plain_move_clears_it() {
        let mut edit = edit("hello");
        edit.move_right(true, false);
        edit.move_right(true, false);
        assert_eq!(edit.selection(), Some((0, 2)));
        edit.move_right(false, false);
        assert!(edit.selection().is_none());
        assert_eq!(edit.display().caret, 3);
    }

    #[test]
    fn undo_restores_the_string_and_redo_puts_it_back() {
        let mut edit = edit("hi");
        edit.insert("!");
        assert_eq!(edit.text(), "!hi");
        assert_eq!(edit.undo(), EditChange::Value);
        assert_eq!(edit.text(), "hi");
        assert_eq!(edit.redo(), EditChange::Value);
        assert_eq!(edit.text(), "!hi");
    }

    #[test]
    fn preedit_is_shown_at_the_caret_until_commit() {
        let mut edit = edit("hello");
        edit.place(2, false);
        edit.set_preedit("ế", None);
        let display = edit.display();
        assert_eq!(display.text, "heếllo");
        assert_eq!(edit.text(), "hello");
        edit.insert("ế");
        assert_eq!(edit.text(), "heếllo");
    }
}
