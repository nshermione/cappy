//! Partial styles that layer on top of each other (theme classes, then inline styles).

use crate::{
    Align, BackgroundValue, ColorValue, Corners, CursorIcon, Dimension, Display, Edges,
    FlexDirection, FontWeight, Justify, Length, Overflow, PointerEvents, Position, ShadowValue,
    Style, TextAlign, Tokens,
};

/// A partial style. Every field is optional; `None` keeps whatever an earlier layer set.
///
/// Values may refer to theme tokens, so the same refinement resolves differently per theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleRefinement {
    pub display: Option<Display>,
    pub direction: Option<FlexDirection>,
    pub wrap: Option<bool>,
    pub justify: Option<Justify>,
    pub align: Option<Align>,
    pub gap: Option<Length>,
    pub padding_top: Option<Length>,
    pub padding_right: Option<Length>,
    pub padding_bottom: Option<Length>,
    pub padding_left: Option<Length>,
    pub margin_top: Option<Length>,
    pub margin_right: Option<Length>,
    pub margin_bottom: Option<Length>,
    pub margin_left: Option<Length>,
    pub width: Option<Dimension>,
    pub height: Option<Dimension>,
    pub min_width: Option<Dimension>,
    pub min_height: Option<Dimension>,
    pub max_width: Option<Dimension>,
    pub max_height: Option<Dimension>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub grid_columns: Option<u16>,
    pub grid_column_span: Option<u16>,
    pub grid_row_span: Option<u16>,
    pub position: Option<Position>,
    pub top: Option<Length>,
    pub right: Option<Length>,
    pub bottom: Option<Length>,
    pub left: Option<Length>,
    pub overflow_x: Option<Overflow>,
    pub overflow_y: Option<Overflow>,
    pub background: Option<BackgroundValue>,
    pub border_color: Option<ColorValue>,
    pub border_top: Option<Length>,
    pub border_right: Option<Length>,
    pub border_bottom: Option<Length>,
    pub border_left: Option<Length>,
    pub radius_top_left: Option<Length>,
    pub radius_top_right: Option<Length>,
    pub radius_bottom_right: Option<Length>,
    pub radius_bottom_left: Option<Length>,
    pub opacity: Option<f32>,
    pub shadow: Option<ShadowValue>,
    pub cursor: Option<CursorIcon>,
    pub pointer_events: Option<PointerEvents>,
    pub text_color: Option<ColorValue>,
    pub font_size: Option<Length>,
    pub line_height: Option<f32>,
    pub font_weight: Option<FontWeight>,
    pub text_align: Option<TextAlign>,
    pub ellipsis: Option<bool>,
}

macro_rules! setters {
    ($($(#[$meta:meta])* $method:ident => $field:ident : $value:ty),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[must_use]
            pub fn $method(mut self, value: impl Into<$value>) -> Self {
                self.$field = Some(value.into());
                self
            }
        )+
    };
}

/// Setters taking the exact type, so plain numeric literals infer without a suffix.
macro_rules! plain_setters {
    ($($(#[$meta:meta])* $method:ident => $field:ident : $value:ty),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[must_use]
            pub fn $method(mut self, value: $value) -> Self {
                self.$field = Some(value);
                self
            }
        )+
    };
}

macro_rules! sided_setters {
    (
        $all:ident, $horizontal:ident, $vertical:ident =>
        $top:ident, $right:ident, $bottom:ident, $left:ident
    ) => {
        #[must_use]
        pub fn $all(self, value: impl Into<Length>) -> Self {
            let value = value.into();
            self.$horizontal(value).$vertical(value)
        }

        #[must_use]
        pub fn $horizontal(mut self, value: impl Into<Length>) -> Self {
            let value = value.into();
            self.$left = Some(value);
            self.$right = Some(value);
            self
        }

        #[must_use]
        pub fn $vertical(mut self, value: impl Into<Length>) -> Self {
            let value = value.into();
            self.$top = Some(value);
            self.$bottom = Some(value);
            self
        }

        setters! {
            $top => $top: Length,
            $right => $right: Length,
            $bottom => $bottom: Length,
            $left => $left: Length,
        }
    };
}

/// Overwrites `$target` with `$source` for every field that `$source` sets.
macro_rules! merge_fields {
    ($target:ident, $source:ident; $($field:ident),+ $(,)?) => {
        $(
            if $source.$field.is_some() {
                $target.$field = $source.$field;
            }
        )+
    };
}

impl StyleRefinement {
    setters! {
        display => display: Display,
        direction => direction: FlexDirection,
        wrap => wrap: bool,
        justify => justify: Justify,
        align => align: Align,
        gap => gap: Length,
        width => width: Dimension,
        height => height: Dimension,
        min_width => min_width: Dimension,
        min_height => min_height: Dimension,
        max_width => max_width: Dimension,
        max_height => max_height: Dimension,
        background => background: BackgroundValue,
        border_color => border_color: ColorValue,
        shadow => shadow: ShadowValue,
        text_color => text_color: ColorValue,
        font_size => font_size: Length,
        font_weight => font_weight: FontWeight,
        text_align => text_align: TextAlign,
        position => position: Position,
        cursor => cursor: CursorIcon,
        pointer_events => pointer_events: PointerEvents,
    }

    plain_setters! {
        flex_grow => flex_grow: f32,
        flex_shrink => flex_shrink: f32,
        grid_columns => grid_columns: u16,
        /// How many equal columns this grid item covers. `0` and `1` are one cell.
        column_span => grid_column_span: u16,
        /// How many rows this grid item covers. `0` and `1` are one cell.
        row_span => grid_row_span: u16,
        /// Multiplier for this node and its descendants. Clamped to `0..=1`.
        opacity => opacity: f32,
        overflow_x => overflow_x: Overflow,
        overflow_y => overflow_y: Overflow,
        /// Line height as a multiple of the font size.
        line_height => line_height: f32,
        /// Clips each explicit line with `…` instead of wrapping.
        ellipsis => ellipsis: bool,
    }

    sided_setters!(padding, padding_x, padding_y =>
        padding_top, padding_right, padding_bottom, padding_left);
    sided_setters!(margin, margin_x, margin_y =>
        margin_top, margin_right, margin_bottom, margin_left);
    sided_setters!(border_width, border_x, border_y =>
        border_top, border_right, border_bottom, border_left);
    sided_setters!(inset, inset_x, inset_y =>
        top, right, bottom, left);

    /// Sets every corner to `value`. One corner can override it afterwards.
    #[must_use]
    pub fn radius(self, value: impl Into<Length>) -> Self {
        let value = value.into();
        self.radius_top_left(value)
            .radius_top_right(value)
            .radius_bottom_right(value)
            .radius_bottom_left(value)
    }

    setters! {
        /// Top-left corner radius.
        radius_top_left => radius_top_left: Length,
        /// Top-right corner radius.
        radius_top_right => radius_top_right: Length,
        /// Bottom-right corner radius.
        radius_bottom_right => radius_bottom_right: Length,
        /// Bottom-left corner radius.
        radius_bottom_left => radius_bottom_left: Length,
    }

    /// Sets [`Overflow`] on both axes.
    #[must_use]
    pub fn overflow(self, value: Overflow) -> Self {
        self.overflow_x(value).overflow_y(value)
    }

    /// Sets width and height together.
    #[must_use]
    pub fn size(self, value: impl Into<Dimension>) -> Self {
        let value = value.into();
        self.width(value).height(value)
    }

    /// Layers `other` on top of `self`: fields set in `other` win.
    pub fn merge(&mut self, other: &StyleRefinement) {
        merge_fields!(self, other;
            display, direction, wrap, justify, align, gap,
            padding_top, padding_right, padding_bottom, padding_left,
            margin_top, margin_right, margin_bottom, margin_left,
            width, height, min_width, min_height, max_width, max_height,
            flex_grow, flex_shrink, grid_columns, grid_column_span, grid_row_span,
            position, top, right, bottom, left, overflow_x, overflow_y,
            background, border_color, border_top, border_right, border_bottom, border_left,
            radius_top_left, radius_top_right, radius_bottom_right, radius_bottom_left,
            opacity, shadow, cursor, pointer_events,
            text_color, font_size, line_height, font_weight, text_align, ellipsis,
        );
    }

    /// Writes every field this refinement sets into `style`, resolving tokens.
    pub fn apply_to(&self, style: &mut Style, tokens: &Tokens) {
        self.apply_layout(style, tokens);
        self.apply_visual(style, tokens);
        self.apply_text(style, tokens);
        if let Some(cursor) = self.cursor {
            style.cursor = Some(cursor);
        }
        if let Some(events) = self.pointer_events {
            style.pointer_events = Some(events);
        }
    }

    fn apply_layout(&self, style: &mut Style, tokens: &Tokens) {
        let layout = &mut style.layout;
        let px = |length: Length| length.resolve(tokens);
        assign(&mut layout.display, self.display);
        assign(&mut layout.direction, self.direction);
        assign(&mut layout.wrap, self.wrap);
        assign(&mut layout.justify, self.justify);
        assign(&mut layout.align, self.align);
        assign(&mut layout.gap, self.gap.map(px));
        apply_edges(
            &mut layout.padding,
            [
                self.padding_top,
                self.padding_right,
                self.padding_bottom,
                self.padding_left,
            ],
            tokens,
        );
        apply_edges(
            &mut layout.margin,
            [
                self.margin_top,
                self.margin_right,
                self.margin_bottom,
                self.margin_left,
            ],
            tokens,
        );
        let extent = |dimension: Dimension| dimension.resolve(tokens);
        assign(&mut layout.width, self.width.map(extent));
        assign(&mut layout.height, self.height.map(extent));
        assign(&mut layout.min_width, self.min_width.map(extent));
        assign(&mut layout.min_height, self.min_height.map(extent));
        assign(&mut layout.max_width, self.max_width.map(extent));
        assign(&mut layout.max_height, self.max_height.map(extent));
        assign(&mut layout.flex_grow, self.flex_grow);
        assign(&mut layout.flex_shrink, self.flex_shrink);
        assign(&mut layout.grid_columns, self.grid_columns);
        assign(&mut layout.grid_column_span, self.grid_column_span);
        assign(&mut layout.grid_row_span, self.grid_row_span);
        assign(&mut layout.position, self.position);
        apply_insets(
            &mut layout.inset,
            [self.top, self.right, self.bottom, self.left],
            tokens,
        );
        assign(&mut layout.overflow_x, self.overflow_x);
        assign(&mut layout.overflow_y, self.overflow_y);
    }

    fn apply_visual(&self, style: &mut Style, tokens: &Tokens) {
        let visual = &mut style.visual;
        assign(
            &mut visual.background,
            self.background.map(|c| c.resolve(tokens)),
        );
        assign(
            &mut visual.border_color,
            self.border_color.map(|c| c.resolve(tokens)),
        );
        apply_edges(
            &mut visual.border_width,
            [
                self.border_top,
                self.border_right,
                self.border_bottom,
                self.border_left,
            ],
            tokens,
        );
        apply_corners(
            &mut visual.corner_radius,
            [
                self.radius_top_left,
                self.radius_top_right,
                self.radius_bottom_right,
                self.radius_bottom_left,
            ],
            tokens,
        );
        if let Some(opacity) = self.opacity {
            visual.opacity = opacity.clamp(0.0, 1.0);
        }
        assign(
            &mut visual.shadow,
            self.shadow.map(|shadow| shadow.resolve(tokens)),
        );
    }

    fn apply_text(&self, style: &mut Style, tokens: &Tokens) {
        let text = &mut style.text;
        assign(&mut text.color, self.text_color.map(|c| c.resolve(tokens)));
        assign(
            &mut text.font_size,
            self.font_size.map(|l| l.resolve(tokens)),
        );
        assign(&mut text.line_height, self.line_height);
        assign(&mut text.weight, self.font_weight);
        assign(&mut text.align, self.text_align);
        assign(&mut text.ellipsis, self.ellipsis);
    }
}

fn assign<T>(target: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *target = value;
    }
}

fn apply_edges(edges: &mut Edges, sides: [Option<Length>; 4], tokens: &Tokens) {
    let [top, right, bottom, left] = sides.map(|side| side.map(|length| length.resolve(tokens)));
    assign(&mut edges.top, top);
    assign(&mut edges.right, right);
    assign(&mut edges.bottom, bottom);
    assign(&mut edges.left, left);
}

fn apply_corners(corners: &mut Corners, sides: [Option<Length>; 4], tokens: &Tokens) {
    let [top_left, top_right, bottom_right, bottom_left] =
        sides.map(|side| side.map(|length| length.resolve(tokens)));
    assign(&mut corners.top_left, top_left);
    assign(&mut corners.top_right, top_right);
    assign(&mut corners.bottom_right, bottom_right);
    assign(&mut corners.bottom_left, bottom_left);
}

fn apply_insets(inset: &mut crate::Insets, sides: [Option<Length>; 4], tokens: &Tokens) {
    let [top, right, bottom, left] = sides.map(|side| side.map(|length| length.resolve(tokens)));
    if let Some(value) = top {
        inset.top = Some(value);
    }
    if let Some(value) = right {
        inset.right = Some(value);
    }
    if let Some(value) = bottom {
        inset.bottom = Some(value);
    }
    if let Some(value) = left {
        inset.left = Some(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, ColorToken, Corners, Edges, Extent, Space, TextStyle};

    fn resolve(refinement: &StyleRefinement, tokens: &Tokens) -> Style {
        let mut style = Style::inheriting(TextStyle::from_tokens(tokens));
        refinement.apply_to(&mut style, tokens);
        style
    }

    #[test]
    fn later_layer_overrides_only_the_fields_it_sets() {
        let mut base = StyleRefinement::default()
            .padding(4)
            .background(Color::BLACK);
        base.merge(&StyleRefinement::default().background(Color::WHITE));
        assert_eq!(base.padding_left, Some(Length::Px(4.0)));
        assert_eq!(
            base.background,
            Some(BackgroundValue::Solid(ColorValue::Literal(Color::WHITE)))
        );
    }

    #[test]
    fn sided_setters_fill_the_right_edges() {
        let tokens = Tokens::light();
        let style = resolve(
            &StyleRefinement::default().padding_x(10).padding_top(2),
            &tokens,
        );
        assert_eq!(
            style.layout.padding,
            Edges {
                top: 2.0,
                right: 10.0,
                bottom: 0.0,
                left: 10.0
            }
        );
    }

    #[test]
    fn one_border_edge_overrides_the_shared_width() {
        let tokens = Tokens::light();
        let style = resolve(
            &StyleRefinement::default()
                .border_width(4)
                .border_top(1)
                .radius(8)
                .radius_top_left(2),
            &tokens,
        );
        assert_eq!(
            style.visual.border_width,
            Edges {
                top: 1.0,
                right: 4.0,
                bottom: 4.0,
                left: 4.0
            }
        );
        assert_eq!(
            style.visual.corner_radius,
            Corners {
                top_left: 2.0,
                top_right: 8.0,
                bottom_right: 8.0,
                bottom_left: 8.0
            }
        );
    }

    #[test]
    fn opacity_clamps_to_the_unit_interval() {
        let tokens = Tokens::light();
        let style = resolve(&StyleRefinement::default().opacity(2.0), &tokens);
        assert_eq!(style.visual.opacity, 1.0);
    }

    #[test]
    fn tokens_resolve_against_the_given_theme() {
        let refinement = StyleRefinement::default()
            .background(ColorToken::Surface)
            .gap(Space::Sm)
            .width(240);
        let light = resolve(&refinement, &Tokens::light());
        let dark = resolve(&refinement, &Tokens::dark());
        assert_eq!(light.visual.background, Tokens::light().colors.surface);
        assert_eq!(dark.visual.background, Tokens::dark().colors.surface);
        assert_eq!(light.layout.gap, 8.0);
        assert_eq!(light.layout.width, Extent::Px(240.0));
        assert_eq!(light.layout, dark.layout);
    }
}
