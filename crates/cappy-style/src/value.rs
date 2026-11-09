//! Style values that are either literal or refer to a theme token.

use std::any::TypeId;

use crate::{Color, StyleToken, Tokens};

/// A color that is either fixed or taken from a theme token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValue {
    Literal(Color),
    Token(TokenColor),
}

/// A color read from a [`StyleToken`] slot when a style is resolved.
#[derive(Clone, Copy)]
pub struct TokenColor {
    index: u8,
    type_id: TypeId,
    read: fn(&Tokens, u8) -> Color,
}

impl PartialEq for TokenColor {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.type_id == other.type_id
    }
}

impl std::fmt::Debug for TokenColor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TokenColor")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl ColorValue {
    pub fn resolve(self, tokens: &Tokens) -> Color {
        match self {
            Self::Literal(color) => color,
            Self::Token(token) => (token.read)(tokens, token.index),
        }
    }
}

impl From<Color> for ColorValue {
    fn from(color: Color) -> Self {
        Self::Literal(color)
    }
}

impl<T> From<T> for ColorValue
where
    T: StyleToken<Value = Color>,
{
    fn from(token: T) -> Self {
        Self::Token(TokenColor {
            index: token.index(),
            type_id: TypeId::of::<T>(),
            read: |tokens, index| T::from_index(index).read(tokens),
        })
    }
}

/// A length in logical pixels, either fixed or taken from a theme token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Token(TokenLength),
}

/// A length read from a [`StyleToken`] slot when a style is resolved.
#[derive(Clone, Copy)]
pub struct TokenLength {
    index: u8,
    type_id: TypeId,
    read: fn(&Tokens, u8) -> f32,
}

impl PartialEq for TokenLength {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.type_id == other.type_id
    }
}

impl std::fmt::Debug for TokenLength {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TokenLength")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl Length {
    pub fn resolve(self, tokens: &Tokens) -> f32 {
        match self {
            Self::Px(value) => value,
            Self::Token(token) => (token.read)(tokens, token.index),
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

impl<T> From<T> for Length
where
    T: StyleToken<Value = f32>,
{
    fn from(token: T) -> Self {
        Self::Token(TokenLength {
            index: token.index(),
            type_id: TypeId::of::<T>(),
            read: |tokens, index| T::from_index(index).read(tokens),
        })
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
    use crate::{ColorToken, FontSize, Radius, Space};

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
