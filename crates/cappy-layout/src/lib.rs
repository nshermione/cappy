//! Geometry, text measurement interface and taffy-based layout for cappy.

mod geometry;
mod text;
mod tree;

pub use geometry::{Point, Rect, Size};
pub use text::{ApproximateTextMeasurer, TextMeasureRequest, TextMeasurer};
pub use tree::{LayoutError, LayoutTree, TextLeaf};
