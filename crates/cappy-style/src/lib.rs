//! Typed styles, design tokens and themes for cappy.
//!
//! - [`StyleRefinement`]: a partial style whose values may reference theme tokens.
//! - [`Theme`]: tokens plus named classes (with `:hover`, `:focus`, `:active` and `:disabled`).
//! - [`Style`]: the fully resolved result used by layout and painting.

mod background;
mod classes;
mod color;
mod refinement;
mod shadow;
mod style;
mod theme;
mod tokens;
mod value;

pub use background::{
    Background, BackgroundValue, GradientDirection, LinearGradient, UnitPoint, linear,
};
pub use classes::Class;
pub use color::{Color, ColorParseError};
pub use refinement::StyleRefinement;
pub use shadow::{BoxShadow, ShadowValue, shadow};
pub use style::{
    Align, Corners, CursorIcon, Display, Edges, FlexDirection, FontWeight, Insets, Justify,
    LayoutStyle, Overflow, PointerEvents, Position, Style, StyleChange, TextAlign, TextStyle,
    VisualStyle, align_offset,
};
pub use theme::{ClassName, InteractionState, Stylesheet, Theme};
pub use tokens::{
    ColorToken, ColorTokenValues, CustomToken, FontSize, FontSizeValues, PatchList, Radius,
    RadiusValues, Space, SpaceValues, StyleToken, Tokens,
};
pub use value::{ColorValue, Dimension, Extent, Length, percent};
