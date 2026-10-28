//! Text shaping shared by measurement and rendering, so both lay text out identically.

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight};

/// Font parameters for one block of text, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextFont {
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
}

/// Builds and shapes a buffer for `text`. `max_width` enables word wrapping.
pub(crate) fn shape_text(
    font_system: &mut FontSystem,
    text: &str,
    font: TextFont,
    max_width: Option<f32>,
) -> Buffer {
    let mut buffer = Buffer::new(font_system, Metrics::new(font.font_size, font.line_height));
    buffer.set_size(max_width, None);
    let attrs = Attrs::new()
        .family(Family::SansSerif)
        .weight(Weight(font.weight));
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
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
