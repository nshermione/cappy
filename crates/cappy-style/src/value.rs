//! Style values that are either literal or refer to a theme token.

use crate::{Color, ColorToken, FontSize, Radius, Space, Tokens};

/// A color that is either fixed or taken from the theme palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValue {
    Literal(Color),
    Token(ColorToken),
}

impl ColorValue {
    pub fn resolve(self, tokens: &Tokens) -> Color {
        match self {
            Self::Literal(color) => color,
            Self::Token(token) => tokens.colors.get(token),
        }
    }
}

impl From<Color> for ColorValue {
    fn from(color: Color) -> Self {
        Self::Literal(color)
    }
}

impl From<ColorToken> for ColorValue {
    fn from(token: ColorToken) -> Self {
        Self::Token(token)
    }
}

/// A length in logical pixels, either fixed or taken from one of the theme scales.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Space(Space),
    Radius(Radius),
    Font(FontSize),
}

impl Length {
    pub fn resolve(self, tokens: &Tokens) -> f32 {
        match self {
            Self::Px(value) => value,
            Self::Space(step) => tokens.spacing.get(step),
            Self::Radius(step) => tokens.radius.get(step),
            Self::Font(step) => tokens.font_sizes.get(step),
        }
    }
}

impl From<f32> for Length {
    fn from(value: f32) -> Self {
        Self::Px(value)
    }
}

impl From<i32> for Length {
    fn from(value: i32) -> Self {
        Self::Px(value as f32)
    }
}

impl From<Space> for Length {
    fn from(step: Space) -> Self {
        Self::Space(step)
    }
}

impl From<Radius> for Length {
    fn from(step: Radius) -> Self {
        Self::Radius(step)
    }
}

impl From<FontSize> for Length {
    fn from(step: FontSize) -> Self {
        Self::Font(step)
    }
}

/// A box dimension as written in a style: `auto`, a length, or a percentage of the parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dimension {
    Auto,
    Length(Length),
    /// Fraction of the parent size, where `100.0` means the full size.
    Percent(f32),
}

/// Shorthand for [`Dimension::Percent`].
pub fn percent(value: f32) -> Dimension {
    Dimension::Percent(value)
}

impl Dimension {
    pub fn resolve(self, tokens: &Tokens) -> Extent {
        match self {
            Self::Auto => Extent::Auto,
            Self::Length(length) => Extent::Px(length.resolve(tokens)),
            Self::Percent(value) => Extent::Percent(value),
        }
    }
}

impl<T: Into<Length>> From<T> for Dimension {
    fn from(value: T) -> Self {
        Self::Length(value.into())
    }
}

/// A resolved box dimension ready for layout.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Extent {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_values_follow_the_theme() {
        let light = Tokens::light();
        let dark = Tokens::dark();
        let background = ColorValue::from(ColorToken::Background);
        assert_eq!(background.resolve(&light), light.colors.background);
        assert_eq!(background.resolve(&dark), dark.colors.background);
    }

    #[test]
    fn lengths_resolve_from_every_scale() {
        let tokens = Tokens::light();
        assert_eq!(Length::from(7).resolve(&tokens), 7.0);
        assert_eq!(Length::from(Space::Lg).resolve(&tokens), tokens.spacing.lg);
        assert_eq!(Length::from(Radius::Sm).resolve(&tokens), tokens.radius.sm);
        assert_eq!(
            Length::from(FontSize::Sm).resolve(&tokens),
            tokens.font_sizes.sm
        );
    }

    #[test]
    fn dimensions_resolve_to_extents() {
        let tokens = Tokens::light();
        assert_eq!(Dimension::from(120).resolve(&tokens), Extent::Px(120.0));
        assert_eq!(percent(50.0).resolve(&tokens), Extent::Percent(50.0));
        assert_eq!(Dimension::Auto.resolve(&tokens), Extent::Auto);
    }
}
