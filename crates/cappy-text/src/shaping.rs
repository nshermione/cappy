//! Text shaping shared by measurement and rendering, so both lay text out identically.

use cappy_core::TextAlign;
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};

/// Font parameters for one block of text, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextFont {
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
}

/// Builds and shapes a buffer for `text`. `max_width` enables word wrapping. `align` shifts each
/// line inside the buffer width. `Start` with no `max_width` leaves the buffer unbounded, which
/// is the single-line path.
pub(crate) fn shape_text(
    font_system: &mut FontSystem,
    text: &str,
    font: TextFont,
    max_width: Option<f32>,
    align: TextAlign,
    align_width: f32,
) -> Buffer {
    let mut buffer = Buffer::new(font_system, Metrics::new(font.font_size, font.line_height));
    let cosmic_align = match align {
        TextAlign::Start => None,
        TextAlign::Center => Some(Align::Center),
        TextAlign::End => Some(Align::End),
    };
    if max_width.is_none() && align != TextAlign::Start {
        buffer.set_wrap(Wrap::None);
        buffer.set_size(Some(align_width), None);
    } else {
        buffer.set_size(max_width, None);
    }
    let attrs = Attrs::new()
        .family(Family::SansSerif)
        .weight(Weight(font.weight));
    buffer.set_text(text, &attrs, Shaping::Advanced, cosmic_align);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

/// Width of the widest line and total height of a shaped buffer.
pub(crate) fn shaped_extent(buffer: &Buffer) -> (f32, f32) {
    buffer
        .layout_runs()
        .fold((0.0_f32, 0.0_f32), |(width, height), run| {
            (width.max(run.line_w), height + run.line_height)
        })
}
