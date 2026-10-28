//! Text measurement interface used by layout.

use crate::Size;

/// Text and font metrics to measure.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasureRequest<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
}

/// Measures text. Implemented by the renderer's font system, or by
/// [`ApproximateTextMeasurer`] where real fonts are unavailable (tests, headless runs).
pub trait TextMeasurer {
    /// Size of the laid-out text. With `max_width`, text wraps at word boundaries.
    fn measure(&mut self, request: &TextMeasureRequest<'_>, max_width: Option<f32>) -> Size;
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
}
