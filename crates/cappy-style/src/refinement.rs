//! Partial styles that layer on top of each other (theme classes, then inline styles).

use crate::{
    Align, ColorValue, Dimension, Display, Edges, FlexDirection, FontWeight, Justify, Length,
    Style, Tokens,
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
    pub background: Option<ColorValue>,
    pub border_color: Option<ColorValue>,
    pub border_width: Option<Length>,
    pub corner_radius: Option<Length>,
    pub text_color: Option<ColorValue>,
    pub font_size: Option<Length>,
    pub line_height: Option<f32>,
    pub font_weight: Option<FontWeight>,
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
        background => background: ColorValue,
        border_color => border_color: ColorValue,
        border_width => border_width: Length,
        /// Corner radius.
        radius => corner_radius: Length,
        text_color => text_color: ColorValue,
        font_size => font_size: Length,
        font_weight => font_weight: FontWeight,
    }

    plain_setters! {
        flex_grow => flex_grow: f32,
        flex_shrink => flex_shrink: f32,
        grid_columns => grid_columns: u16,
        /// Line height as a multiple of the font size.
        line_height => line_height: f32,
    }

    sided_setters!(padding, padding_x, padding_y =>
        padding_top, padding_right, padding_bottom, padding_left);
    sided_setters!(margin, margin_x, margin_y =>
        margin_top, margin_right, margin_bottom, margin_left);

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
            flex_grow, flex_shrink, grid_columns,
            background, border_color, border_width, corner_radius,
            text_color, font_size, line_height, font_weight,
        );
    }

    /// Writes every field this refinement sets into `style`, resolving tokens.
    pub fn apply_to(&self, style: &mut Style, tokens: &Tokens) {
        self.apply_layout(style, tokens);
        self.apply_visual(style, tokens);
        self.apply_text(style, tokens);
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
        assign(
            &mut visual.border_width,
            self.border_width.map(|l| l.resolve(tokens)),
        );
        assign(
            &mut visual.corner_radius,
            self.corner_radius.map(|l| l.resolve(tokens)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, ColorToken, Extent, Space, TextStyle};

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
        assert_eq!(base.background, Some(ColorValue::Literal(Color::WHITE)));
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
