//! Caret and selection geometry for a buffer [`shape_text`](crate::shaping::shape_text) already
//! laid out. Indexes are byte offsets into that buffer's text, on grapheme boundaries.

use cosmic_text::{Buffer, LayoutGlyph};

/// Byte index nearest to `(x, y)` in buffer space. `x` before the first glyph is 0. `x` past the
/// line is the end of that line. The caret sits in the gap closest to `x`: the left half of a
/// glyph belongs to the gap before it, the right half to the gap after it.
pub fn hit_index(buffer: &Buffer, x: f32, y: f32) -> usize {
    let mut runs: Vec<_> = buffer.layout_runs().collect();
    if runs.is_empty() {
        return 0;
    }
    let line = runs
        .iter()
        .position(|run| y < run.line_top + run.line_height)
        .unwrap_or(runs.len() - 1);
    let run = runs.swap_remove(line);
    hit_line(&run.glyphs, x).unwrap_or_else(|| end_of(run.glyphs))
}

/// Left edge of the caret at `index`, plus the line's top and height, in buffer space.
pub fn caret_origin(buffer: &Buffer, index: usize) -> Option<(f32, f32, f32)> {
    for run in buffer.layout_runs() {
        let on_line = run.glyphs.iter().any(|glyph| index <= glyph.end) || run.glyphs.is_empty();
        if !on_line {
            continue;
        }
        let x = caret_x(&run.glyphs, index);
        return Some((x, run.line_top, run.line_height));
    }
    None
}

/// Rectangles covering glyphs that overlap `start..end`, in buffer space: x, top, width, height.
pub fn selection_spans(buffer: &Buffer, start: usize, end: usize) -> Vec<(f32, f32, f32, f32)> {
    if start >= end {
        return Vec::new();
    }
    let mut spans = Vec::new();
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            if glyph.end <= start || glyph.start >= end {
                continue;
            }
            spans.push((glyph.x, run.line_top, glyph.w, run.line_height));
        }
    }
    spans
}

fn hit_line(glyphs: &[LayoutGlyph], x: f32) -> Option<usize> {
    if glyphs.is_empty() {
        return Some(0);
    }
    for glyph in glyphs {
        let mid = glyph.x + glyph.w * 0.5;
        if x < mid {
            return Some(if glyph.level.is_rtl() {
                glyph.end
            } else {
                glyph.start
            });
        }
    }
    None
}

fn caret_x(glyphs: &[LayoutGlyph], index: usize) -> f32 {
    if glyphs.is_empty() {
        return 0.0;
    }
    for glyph in glyphs {
        if index <= glyph.start {
            return if glyph.level.is_rtl() {
                glyph.x + glyph.w
            } else {
                glyph.x
            };
        }
        if index < glyph.end {
            return if glyph.level.is_rtl() {
                glyph.x
            } else {
                glyph.x + glyph.w
            };
        }
    }
    end_x(glyphs)
}

fn end_of(glyphs: &[LayoutGlyph]) -> usize {
    glyphs.last().map(|glyph| glyph.end).unwrap_or(0)
}

fn end_x(glyphs: &[LayoutGlyph]) -> f32 {
    let Some(last) = glyphs.last() else {
        return 0.0;
    };
    if last.level.is_rtl() {
        last.x
    } else {
        last.x + last.w
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use cosmic_text::FontSystem;

    use super::*;
    use crate::font::FontContext;
    use crate::shaping::{TextFont, shape_text};

    fn font_system() -> FontSystem {
        static FONTS: OnceLock<FontContext> = OnceLock::new();
        FONTS.get_or_init(FontContext::from_system).font_system()
    }

    fn shaped(text: &str) -> Buffer {
        shape_text(
            &mut font_system(),
            text,
            TextFont {
                font_size: 16.0,
                line_height: 20.0,
                weight: 400,
            },
            None,
        )
    }

    #[test]
    fn the_edges_of_a_line_map_to_the_start_and_end() {
        let buffer = shaped("hello");
        assert_eq!(hit_index(&buffer, -20.0, 1.0), 0);
        assert_eq!(hit_index(&buffer, 10_000.0, 1.0), "hello".len());
        let start = caret_origin(&buffer, 0).expect("caret at the start");
        let end = caret_origin(&buffer, "hello".len()).expect("caret at the end");
        assert!(end.0 >= start.0);
        assert!(!selection_spans(&buffer, 1, 4).is_empty());
    }
}
