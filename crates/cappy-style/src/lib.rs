//! Typed styles, design tokens and themes for cappy.
//!
//! - [`StyleRefinement`]: a partial style whose values may reference theme tokens.
//! - [`Theme`]: tokens plus named classes (with `:hover` / `:active` variants).
//! - [`Style`]: the fully resolved result used by layout and painting.

mod color;
mod refinement;
mod style;
mod theme;
mod tokens;
mod value;

pub use color::{Color, ColorParseError};
pub use refinement::StyleRefinement;
pub use style::{
    Align, Display, Edges, FlexDirection, FontWeight, Justify, LayoutStyle, Style, StyleChange,
    TextStyle, VisualStyle,
};
pub use theme::{ClassName, InteractionState, Theme};
pub use tokens::{
    ColorToken, FontSize, FontSizeScale, Palette, Radius, RadiusScale, Space, SpacingScale, Tokens,
};
pub use value::{ColorValue, Dimension, Extent, Length, percent};
