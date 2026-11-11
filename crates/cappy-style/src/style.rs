//! Fully resolved styles: concrete values only, no tokens.

use crate::{Background, BoxShadow, Color, Extent, Tokens};

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

/// Whether a box shows content outside its bounds, and whether the wheel can move that content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Children paint outside the box and the box grows with them.
    #[default]
    Visible,
    /// Children are clipped to the box. The wheel does not move them.
    Hidden,
    /// Children are clipped. The wheel moves them when they are larger than the box.
    Auto,
}

impl Overflow {
    /// Content outside the box is clipped.
    pub const fn clips(self) -> bool {
        matches!(self, Self::Hidden | Self::Auto)
    }

    /// The wheel may change this box's scroll offset.
    pub const fn scrolls(self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// Horizontal alignment of a line inside its box. `Start` is the current left edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Cursor the host shows while the pointer is over a node that sets one.
///
/// `None` on a resolved style leaves the cursor to click and text. `Resize` is the diagonal
/// northwest-southeast arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorIcon {
    #[default]
    Default,
    Pointer,
    /// I-beam over text that can take a caret.
    Text,
    /// Diagonal resize arrow.
    Resize,
    /// Open hand, for something that can be dragged.
    Grab,
    /// The action under the pointer is not available.
    NotAllowed,
}

/// Whether a node is a hit target. `None` on a resolved style means the node inherits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PointerEvents {
    /// The node is a hit target.
    #[default]
    Auto,
    /// The node is not a hit target. Descendants inherit this until one sets [`Self::Auto`].
    None,
}

/// How a box is taken out of the scroll, if at all.
///
/// [`Self::Sticky`] stays in normal flow. The runtime shifts it against the nearest scrolling
/// ancestor. Insets are scroll thresholds, not a layout offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Sticky,
}

/// Optional per-side offsets. `None` means the side is not set.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Insets {
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub left: Option<f32>,
}

impl Insets {
    /// True when every side is unset.
    pub const fn is_auto(self) -> bool {
        self.top.is_none() && self.right.is_none() && self.bottom.is_none() && self.left.is_none()
    }
}

/// Extra space on the left of a line that is narrower than `box_width`. A wider line stays at 0
/// so scrolling, not alignment, keeps the caret in view.
pub fn align_offset(box_width: f32, line_width: f32, align: TextAlign) -> f32 {
    let extra = (box_width - line_width).max(0.0);
    match align {
        TextAlign::Start => 0.0,
        TextAlign::Center => extra * 0.5,
        TextAlign::End => extra,
    }
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

    /// True when any side is wider than zero.
    pub fn any_positive(self) -> bool {
        self.top > 0.0 || self.right > 0.0 || self.bottom > 0.0 || self.left > 0.0
    }

    /// True when every side is the same value.
    pub fn is_uniform(self) -> bool {
        self.right == self.top && self.bottom == self.top && self.left == self.top
    }

    /// Scales every side. Used when a transformed box paints.
    pub fn scaled(self, factor: f32) -> Self {
        Self {
            top: self.top * factor,
            right: self.right * factor,
            bottom: self.bottom * factor,
            left: self.left * factor,
        }
    }
}

impl std::ops::MulAssign<f32> for Edges {
    fn mul_assign(&mut self, factor: f32) {
        *self = self.scaled(factor);
    }
}

/// A radius for each corner, in logical pixels. Order is clockwise from the top left.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Corners {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl Corners {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }

    /// True when every corner is the same value.
    pub fn is_uniform(self) -> bool {
        self.top_right == self.top_left
            && self.bottom_right == self.top_left
            && self.bottom_left == self.top_left
    }

    /// The largest corner. Image clipping uses one radius, and a shared `radius` stays that value.
    pub fn max_radius(self) -> f32 {
        self.top_left
            .max(self.top_right)
            .max(self.bottom_right)
            .max(self.bottom_left)
    }

    /// Scales every corner. Used when a transformed box paints.
    pub fn scaled(self, factor: f32) -> Self {
        Self {
            top_left: self.top_left * factor,
            top_right: self.top_right * factor,
            bottom_right: self.bottom_right * factor,
            bottom_left: self.bottom_left * factor,
        }
    }
}

impl std::ops::MulAssign<f32> for Corners {
    fn mul_assign(&mut self, factor: f32) {
        *self = self.scaled(factor);
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
    /// How many columns this item covers. `0` and `1` are a single cell.
    pub grid_column_span: u16,
    /// How many rows this item covers. `0` and `1` are a single cell.
    pub grid_row_span: u16,
    pub position: Position,
    /// Sticky thresholds. Ignored unless [`Self::position`] is [`Position::Sticky`].
    pub inset: Insets,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
}

impl LayoutStyle {
    /// Either axis clips children to this box.
    pub const fn clips(&self) -> bool {
        self.overflow_x.clips() || self.overflow_y.clips()
    }
}

/// Properties that only affect painting.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualStyle {
    pub background: Background,
    pub border_color: Color,
    /// Width of each border edge. A uniform value is the old single width.
    pub border_width: Edges,
    /// Radius of each corner. A uniform value is the old single radius.
    pub corner_radius: Corners,
    pub shadow: BoxShadow,
    /// Multiplier for this node and its descendants, clamped to `0..=1`. Identity is 1.
    /// Motion opacity multiplies on top of this.
    pub opacity: f32,
}

impl Default for VisualStyle {
    fn default() -> Self {
        Self {
            background: Background::Solid(Color::TRANSPARENT),
            border_color: Color::TRANSPARENT,
            border_width: Edges::ZERO,
            corner_radius: Corners::ZERO,
            shadow: BoxShadow::NONE,
            opacity: 1.0,
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
    pub align: TextAlign,
    /// When set, text stays one line per explicit newline and the paint clips the tail with `…`.
    pub ellipsis: bool,
}

impl TextStyle {
    pub fn from_tokens(tokens: &Tokens) -> Self {
        Self {
            color: tokens.colors.text,
            font_size: tokens.font_sizes.md,
            line_height: tokens.line_height,
            weight: FontWeight::NORMAL,
            align: TextAlign::Start,
            ellipsis: false,
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
            || self.ellipsis != other.ellipsis
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
    /// When set, the pointer shows this cursor instead of the one inferred from the node.
    /// Not inherited. The deepest set value on the hovered path wins.
    pub cursor: Option<CursorIcon>,
    /// `None` inherits [`PointerEvents`] from the parent. The root's `None` is [`PointerEvents::Auto`].
    pub pointer_events: Option<PointerEvents>,
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
            cursor: None,
            pointer_events: None,
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
    use crate::shadow;

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
        changed.visual.background = Background::Solid(Color::BLACK);
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
    fn shadow_change_only_needs_paint() {
        let mut changed = base();
        changed.visual.shadow = shadow(0, 4, 8, 0, Color::BLACK).resolve(&Tokens::light());
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

    #[test]
    fn text_align_repaints_and_is_inherited_without_layout() {
        let mut changed = base();
        changed.text.align = TextAlign::Center;
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
    fn opacity_repaints_without_layout() {
        let mut changed = base();
        changed.visual.opacity = 0.5;
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
    fn a_single_border_edge_only_needs_paint() {
        let mut changed = base();
        changed.visual.border_width.top = 2.0;
        assert!(changed.diff(&base()).paint);
        assert!(!changed.diff(&base()).layout);
    }

    #[test]
    fn cursor_and_pointer_events_do_not_invalidate_paint_or_layout() {
        let mut changed = base();
        changed.cursor = Some(CursorIcon::Grab);
        changed.pointer_events = Some(PointerEvents::None);
        assert!(!changed.diff(&base()).any());
    }

    #[test]
    fn column_span_needs_layout() {
        let mut changed = base();
        changed.layout.grid_column_span = 2;
        assert!(changed.diff(&base()).layout);
        assert!(!changed.diff(&base()).paint);
    }

    #[test]
    fn ellipsis_changes_the_measured_height() {
        let mut changed = base();
        changed.text.ellipsis = true;
        let change = changed.diff(&base());
        assert!(change.layout && change.inherited);
    }

    #[test]
    fn a_centered_line_shifts_by_half_the_leftover_width() {
        assert_eq!(align_offset(100.0, 40.0, TextAlign::Start), 0.0);
        assert_eq!(align_offset(100.0, 40.0, TextAlign::Center), 30.0);
        assert_eq!(align_offset(100.0, 40.0, TextAlign::End), 60.0);
        assert_eq!(align_offset(40.0, 100.0, TextAlign::Center), 0.0);
    }
}
