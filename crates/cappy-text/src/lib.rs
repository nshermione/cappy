//! Text support shared by every cappy renderer: the font database, shaping with cosmic-text,
//! the layout measurer, and a cache of shaped buffers, so the GPU and CPU renderers lay text
//! out exactly like layout measured it.

mod cache;
mod font;
mod measure;
mod shaping;

pub use cache::ShapedTextCache;
pub use font::FontContext;
pub use measure::CosmicTextMeasurer;

pub use cosmic_text;
pub use cosmic_text::fontdb;
