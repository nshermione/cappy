//! Text support shared by layout and the renderer: the font database, shaping with cosmic-text,
//! the layout measurer, and a cache of shaped buffers, so drawing matches what layout measured.

mod cache;
mod font;
mod geometry;
mod measure;
mod shaping;

pub use cache::ShapedTextCache;
pub use font::{FontContext, FontOptions};
pub use geometry::{caret_origin, hit_index, selection_spans};
pub use measure::CosmicTextMeasurer;

pub use cosmic_text;
pub use cosmic_text::fontdb;
