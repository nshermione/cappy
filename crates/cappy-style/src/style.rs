//! Fully resolved styles: concrete values only, no tokens.

use crate::{Color, Extent, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Display {
    #[default]
    Flex,
    Grid,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
}

/// Distribution of children along the main axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Placement of children along the cross axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const NORMAL: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const SEMIBOLD: Self = Self(600);
    pub const BOLD: Self = Self(700);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// Per-side values in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Properties that affect layout.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LayoutStyle {
    pub display: Display,
    pub direction: FlexDirection,
    pub wrap: bool,
    pub justify: Justify,
    pub align: Align,
    pub gap: f32,
    pub padding: Edges,
    pub margin: Edges,
    pub width: Extent,
    pub height: Extent,
    pub min_width: Extent,
    pub min_height: Extent,
    pub max_width: Extent,
    pub max_height: Extent,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    /// Number of equal grid columns when `display` is [`Display::Grid`].
    pub grid_columns: u16,
}

/// Properties that only affect painting.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualStyle {
    pub background: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub corner_radius: f32,
}

impl Default for VisualStyle {
    fn default() -> Self {
        Self {
            background: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            corner_radius: 0.0,
        }
    }
}

/// Text properties. These are inherited by descendants unless overridden.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    pub font_size: f32,
    /// Line height as a multiple of `font_size`.
    pub line_height: f32,
    pub weight: FontWeight,
}

impl TextStyle {
    pub fn from_tokens(tokens: &Tokens) -> Self {
        Self {
            color: tokens.colors.text,
            font_size: tokens.font_sizes.md,
            line_height: tokens.line_height,
            weight: FontWeight::NORMAL,
        }
    }

    pub fn line_height_px(&self) -> f32 {
        self.font_size * self.line_height
    }

    /// True when the change would alter the measured size of text.
    pub fn metrics_differ(&self, other: &Self) -> bool {
        self.font_size != other.font_size
            || self.line_height != other.line_height
            || self.weight != other.weight
    }
}

/// What changed between two resolved styles, which tells the runtime how much work to redo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StyleChange {
    pub layout: bool,
    pub paint: bool,
    /// Inherited text properties changed, so descendants must be re-resolved.
    pub inherited: bool,
}

impl StyleChange {
    pub fn any(&self) -> bool {
        self.layout || self.paint || self.inherited
    }
}

/// A fully resolved style.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub layout: LayoutStyle,
    pub visual: VisualStyle,
    pub text: TextStyle,
}

impl Style {
    /// Style a node starts from before classes apply: layout and visuals reset,
    /// text inherited from the parent.
    pub fn inheriting(text: TextStyle) -> Self {
        Self {
            layout: LayoutStyle {
                flex_shrink: 1.0,
                ..LayoutStyle::default()
            },
            visual: VisualStyle::default(),
            text,
        }
    }

    pub fn diff(&self, previous: &Self) -> StyleChange {
        let text_metrics_changed = self.text.metrics_differ(&previous.text);
        StyleChange {
            layout: self.layout != previous.layout || text_metrics_changed,
            paint: self.visual != previous.visual || self.text != previous.text,
            inherited: self.text != previous.text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Style {
        Style::inheriting(TextStyle::from_tokens(&Tokens::light()))
    }

    #[test]
    fn identical_styles_report_no_change() {
        assert!(!base().diff(&base()).any());
    }

    #[test]
    fn background_change_only_needs_paint() {
        let mut changed = base();
        changed.visual.background = Color::BLACK;
        assert_eq!(
            changed.diff(&base()),
            StyleChange {
                layout: false,
                paint: true,
                inherited: false
            }
        );
    }

    #[test]
    fn padding_change_needs_layout() {
        let mut changed = base();
        changed.layout.padding = Edges::all(4.0);
        assert!(changed.diff(&base()).layout);
    }

    #[test]
    fn text_color_is_inherited_but_keeps_layout() {
        let mut changed = base();
        changed.text.color = Color::WHITE;
        assert_eq!(
            changed.diff(&base()),
            StyleChange {
                layout: false,
                paint: true,
                inherited: true
            }
        );
    }

    #[test]
    fn font_size_change_needs_layout() {
        let mut changed = base();
        changed.text.font_size = 30.0;
        let change = changed.diff(&base());
        assert!(change.layout && change.inherited);
    }
}
