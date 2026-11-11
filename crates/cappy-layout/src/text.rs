//! Text measurement interface used by layout.

use cappy_style::{TextAlign, align_offset};

use crate::Size;

/// Text and font metrics to measure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMeasureRequest<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
}

/// Where a caret sits in the text's own coordinates, before the box origin and scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretGeometry {
    /// Left edge of the caret. Includes [`TextAlign`] inside the alignment box.
    pub x: f32,
    /// Top of the caret's line.
    pub y: f32,
    /// Width of that line before alignment.
    pub line_width: f32,
    pub line_height: f32,
}

/// Measures text. Implemented by the renderer's font system, or by
/// [`ApproximateTextMeasurer`] where real fonts are unavailable (tests, headless runs).
pub trait TextMeasurer {
    /// Size of the laid-out text. With `max_width`, text wraps at word boundaries.
    fn measure(&mut self, request: &TextMeasureRequest<'_>, max_width: Option<f32>) -> Size;

    /// Byte index of the gap nearest to `(x, y)` in the text's own coordinates. The left half of
    /// a glyph maps to the gap before it and the right half to the gap after it. `y` picks the
    /// line. `max_width` wraps the same way [`measure`](Self::measure) does; `None` is one line
    /// per explicit newline. `align` shifts each line inside `align_width` before the hit.
    fn hit_test(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
        x: f32,
        y: f32,
    ) -> usize;

    /// Caret geometry for the gap at `index`. `index` is a byte offset, clamped to the text.
    fn caret_geometry(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
        index: usize,
    ) -> CaretGeometry;

    /// Cached measurements this measurer is still holding.
    fn cached_measurements(&self) -> usize {
        0
    }
}

/// Deterministic measurer that treats every character as `font_size * advance_ratio` wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApproximateTextMeasurer {
    pub advance_ratio: f32,
}

const DEFAULT_ADVANCE_RATIO: f32 = 0.5;

impl Default for ApproximateTextMeasurer {
    fn default() -> Self {
        Self {
            advance_ratio: DEFAULT_ADVANCE_RATIO,
        }
    }
}

impl TextMeasurer for ApproximateTextMeasurer {
    fn measure(&mut self, request: &TextMeasureRequest<'_>, max_width: Option<f32>) -> Size {
        let advance = request.font_size * self.advance_ratio;
        let width_of = |chars: usize| chars as f32 * advance;
        let mut widest_line: f32 = 0.0;
        let mut line_count = 0usize;
        for paragraph in request.text.split('\n') {
            let lines = wrap_words(paragraph, max_width.map(|width| (width / advance).floor()));
            for line_chars in lines {
                widest_line = widest_line.max(width_of(line_chars));
                line_count += 1;
            }
        }
        Size::new(widest_line, line_count as f32 * request.line_height)
    }

    fn hit_test(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
        x: f32,
        y: f32,
    ) -> usize {
        let advance = request.font_size * self.advance_ratio;
        let line_height = request.line_height.max(1.0);
        let lines = line_ranges(request.text, max_chars(max_width, advance));
        let line_index = line_index_at_y(y, line_height, lines.len());
        let line = &lines[line_index];
        let shift = align_offset(align_width, line.chars as f32 * advance, align);
        line.start + hit_line(&request.text[line.start..line.end], advance, x - shift)
    }

    fn caret_geometry(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
        index: usize,
    ) -> CaretGeometry {
        let advance = request.font_size * self.advance_ratio;
        let line_height = request.line_height.max(1.0);
        let lines = line_ranges(request.text, max_chars(max_width, advance));
        let line_index = line_index_at_byte(index, &lines);
        let line = &lines[line_index];
        let within = chars_before(request.text, line, index);
        let line_width = line.chars as f32 * advance;
        CaretGeometry {
            x: within as f32 * advance + align_offset(align_width, line_width, align),
            y: line_index as f32 * line_height,
            line_width,
            line_height,
        }
    }
}

/// Gap nearest to `x` inside one line. Each character is `advance` wide.
fn hit_line(text: &str, advance: f32, x: f32) -> usize {
    if advance <= 0.0 {
        return 0;
    }
    let mut cursor = 0.0;
    for (offset, _) in text.char_indices() {
        if x < cursor + advance * 0.5 {
            return offset;
        }
        cursor += advance;
    }
    text.len()
}

fn max_chars(max_width: Option<f32>, advance: f32) -> Option<f32> {
    max_width.map(|width| {
        if advance <= 0.0 {
            0.0
        } else {
            (width / advance).floor()
        }
    })
}

fn line_index_at_y(y: f32, line_height: f32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let index = (y / line_height).floor().max(0.0) as usize;
    index.min(count - 1)
}

fn line_index_at_byte(index: usize, lines: &[VisualLine]) -> usize {
    let mut found = 0usize;
    for (position, line) in lines.iter().enumerate() {
        if line.start <= index {
            found = position;
        } else {
            break;
        }
    }
    found
}

fn chars_before(text: &str, line: &VisualLine, index: usize) -> usize {
    if index <= line.start {
        0
    } else if index >= line.end {
        line.chars
    } else {
        text[line.start..index].chars().count()
    }
}

struct VisualLine {
    start: usize,
    end: usize,
    chars: usize,
}

/// Visual lines of `text`. `max_chars` wraps at word boundaries the same way [`wrap_words`] does.
fn line_ranges(text: &str, max_chars: Option<f32>) -> Vec<VisualLine> {
    let mut lines = Vec::new();
    let mut base = 0usize;
    for paragraph in text.split('\n') {
        lines.extend(paragraph_lines(paragraph, base, max_chars));
        base += paragraph.len() + 1;
    }
    if lines.is_empty() {
        lines.push(VisualLine {
            start: 0,
            end: 0,
            chars: 0,
        });
    }
    lines
}

fn paragraph_lines(paragraph: &str, base: usize, max_chars: Option<f32>) -> Vec<VisualLine> {
    let Some(max_chars) = max_chars else {
        return vec![VisualLine {
            start: base,
            end: base + paragraph.len(),
            chars: paragraph.chars().count(),
        }];
    };
    let words = word_spans(paragraph);
    if words.is_empty() {
        return vec![VisualLine {
            start: base,
            end: base + paragraph.len(),
            chars: 0,
        }];
    }
    let mut lines = Vec::new();
    let mut current_chars = words[0].2;
    let mut line_start = words[0].0;
    let mut line_end = words[0].1;
    for &(start, end, chars) in &words[1..] {
        let with_word = current_chars + 1 + chars;
        if with_word as f32 > max_chars {
            lines.push(VisualLine {
                start: base + line_start,
                end: base + line_end,
                chars: current_chars,
            });
            line_start = start;
            line_end = end;
            current_chars = chars;
        } else {
            line_end = end;
            current_chars = with_word;
        }
    }
    lines.push(VisualLine {
        start: base + line_start,
        end: base + line_end,
        chars: current_chars,
    });
    lines
}

/// `(start, end, char count)` of each word in `paragraph`, in byte offsets.
fn word_spans(paragraph: &str) -> Vec<(usize, usize, usize)> {
    let mut words = Vec::new();
    let mut offset = 0usize;
    let mut rest = paragraph;
    while let Some(relative) = rest.find(|ch: char| !ch.is_whitespace()) {
        let start = offset + relative;
        let word = &paragraph[start..];
        let len = word.find(char::is_whitespace).unwrap_or(word.len());
        words.push((start, start + len, word[..len].chars().count()));
        offset = start + len;
        rest = &paragraph[offset..];
        if rest.is_empty() {
            break;
        }
    }
    words
}

/// Greedy word wrap. Returns the character count of each produced line.
fn wrap_words(paragraph: &str, max_chars: Option<f32>) -> Vec<usize> {
    let Some(max_chars) = max_chars else {
        return vec![paragraph.chars().count()];
    };
    let mut lines = Vec::new();
    let mut current = 0usize;
    for word in paragraph.split_whitespace() {
        let word_chars = word.chars().count();
        let with_word = if current == 0 {
            word_chars
        } else {
            current + 1 + word_chars
        };
        if current > 0 && with_word as f32 > max_chars {
            lines.push(current);
            current = word_chars;
        } else {
            current = with_word;
        }
    }
    lines.push(current);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str) -> TextMeasureRequest<'_> {
        TextMeasureRequest {
            text,
            font_size: 10.0,
            line_height: 14.0,
            weight: 400,
        }
    }

    #[test]
    fn single_line_without_constraint() {
        let size = ApproximateTextMeasurer::default().measure(&request("hello"), None);
        assert_eq!(size, Size::new(25.0, 14.0));
    }

    #[test]
    fn wraps_at_word_boundaries() {
        let size = ApproximateTextMeasurer::default().measure(&request("aaa bbb ccc"), Some(40.0));
        assert_eq!(size, Size::new(35.0, 28.0));
    }

    #[test]
    fn zero_width_gives_min_content() {
        let size = ApproximateTextMeasurer::default().measure(&request("aa bbbb c"), Some(0.0));
        assert_eq!(size, Size::new(20.0, 42.0));
    }

    #[test]
    fn explicit_newlines_start_new_lines() {
        let size = ApproximateTextMeasurer::default().measure(&request("ab\ncdef"), None);
        assert_eq!(size, Size::new(20.0, 28.0));
    }

    #[test]
    fn a_click_between_characters_lands_in_that_gap() {
        let mut measurer = ApproximateTextMeasurer::default();
        let request = request("hello");
        assert_eq!(
            measurer.hit_test(&request, None, TextAlign::Start, 0.0, -4.0, 0.0),
            0
        );
        assert_eq!(
            measurer.hit_test(&request, None, TextAlign::Start, 0.0, 11.0, 0.0),
            2
        );
        assert_eq!(
            measurer.hit_test(&request, None, TextAlign::Start, 0.0, 80.0, 0.0),
            5
        );
    }

    #[test]
    fn a_click_on_the_second_wrapped_line_lands_in_that_line() {
        let mut measurer = ApproximateTextMeasurer::default();
        let request = request("aaa bbb ccc");
        let index = measurer.hit_test(&request, Some(40.0), TextAlign::Start, 40.0, 1.0, 16.0);
        assert!(request.text[index..].starts_with("ccc") || index == request.text.len());
        assert!(index > "aaa bbb".len());
    }

    #[test]
    fn the_caret_on_the_second_line_starts_one_line_down() {
        let mut measurer = ApproximateTextMeasurer::default();
        let request = request("ab\ncd");
        let caret = measurer.caret_geometry(&request, None, TextAlign::Start, 0.0, 3);
        assert_eq!(caret.y, 14.0);
        assert_eq!(caret.x, 0.0);
    }

    #[test]
    fn centering_shifts_the_caret_by_half_the_leftover_width() {
        let mut measurer = ApproximateTextMeasurer::default();
        let request = request("aa");
        let caret = measurer.caret_geometry(&request, None, TextAlign::Center, 40.0, 0);
        assert_eq!(caret.x, 15.0);
    }
}
