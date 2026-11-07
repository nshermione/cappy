//! Background brushes: a solid color or a two-stop linear gradient.

use crate::{Color, ColorValue, Tokens};

/// A point in the border box, where `0` is the top or left edge and `1` is the opposite edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitPoint {
    pub x: f32,
    pub y: f32,
}

impl UnitPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Which way a linear gradient runs across the border box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GradientDirection {
    ToRight,
    ToLeft,
    ToTop,
    #[default]
    ToBottom,
}

impl GradientDirection {
    /// Start and end in unit-box coordinates. Y grows downward.
    pub const fn points(self) -> (UnitPoint, UnitPoint) {
        match self {
            Self::ToRight => (UnitPoint::new(0.0, 0.5), UnitPoint::new(1.0, 0.5)),
            Self::ToLeft => (UnitPoint::new(1.0, 0.5), UnitPoint::new(0.0, 0.5)),
            Self::ToTop => (UnitPoint::new(0.5, 1.0), UnitPoint::new(0.5, 0.0)),
            Self::ToBottom => (UnitPoint::new(0.5, 0.0), UnitPoint::new(0.5, 1.0)),
        }
    }
}

/// A two-stop linear gradient whose colors may still be theme tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearGradient {
    pub from: ColorValue,
    pub to: ColorValue,
    pub direction: GradientDirection,
}

impl LinearGradient {
    #[must_use]
    pub fn to_right(mut self) -> Self {
        self.direction = GradientDirection::ToRight;
        self
    }

    #[must_use]
    pub fn to_left(mut self) -> Self {
        self.direction = GradientDirection::ToLeft;
        self
    }

    #[must_use]
    pub fn to_top(mut self) -> Self {
        self.direction = GradientDirection::ToTop;
        self
    }

    #[must_use]
    pub fn to_bottom(mut self) -> Self {
        self.direction = GradientDirection::ToBottom;
        self
    }
}

/// A linear gradient from `from` to `to`, running toward the bottom until a direction is set.
pub fn linear(from: impl Into<ColorValue>, to: impl Into<ColorValue>) -> LinearGradient {
    LinearGradient {
        from: from.into(),
        to: to.into(),
        direction: GradientDirection::ToBottom,
    }
}

/// A background before theme tokens are resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundValue {
    Solid(ColorValue),
    Linear(LinearGradient),
}

impl BackgroundValue {
    pub fn resolve(self, tokens: &Tokens) -> Background {
        match self {
            Self::Solid(color) => Background::Solid(color.resolve(tokens)),
            Self::Linear(gradient) => {
                let (start, end) = gradient.direction.points();
                Background::Linear {
                    from: gradient.from.resolve(tokens),
                    to: gradient.to.resolve(tokens),
                    start,
                    end,
                }
            }
        }
    }
}

impl From<Color> for BackgroundValue {
    fn from(color: Color) -> Self {
        Self::Solid(ColorValue::from(color))
    }
}

impl From<crate::ColorToken> for BackgroundValue {
    fn from(token: crate::ColorToken) -> Self {
        Self::Solid(ColorValue::from(token))
    }
}

impl From<ColorValue> for BackgroundValue {
    fn from(color: ColorValue) -> Self {
        Self::Solid(color)
    }
}

impl From<LinearGradient> for BackgroundValue {
    fn from(gradient: LinearGradient) -> Self {
        Self::Linear(gradient)
    }
}

/// A resolved background: one solid color, or a linear blend of two colors across the border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Background {
    Solid(Color),
    Linear {
        from: Color,
        to: Color,
        start: UnitPoint,
        end: UnitPoint,
    },
}

impl Default for Background {
    fn default() -> Self {
        Self::Solid(Color::TRANSPARENT)
    }
}

impl Background {
    pub fn is_transparent(self) -> bool {
        match self {
            Self::Solid(color) => color.is_transparent(),
            Self::Linear { from, to, .. } => from.is_transparent() && to.is_transparent(),
        }
    }
}

impl From<Color> for Background {
    fn from(color: Color) -> Self {
        Self::Solid(color)
    }
}

impl PartialEq<Color> for Background {
    fn eq(&self, other: &Color) -> bool {
        matches!(self, Self::Solid(color) if color == other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorToken, Tokens};

    #[test]
    fn directions_cross_the_unit_box() {
        assert_eq!(
            GradientDirection::ToRight.points(),
            (UnitPoint::new(0.0, 0.5), UnitPoint::new(1.0, 0.5))
        );
        assert_eq!(
            GradientDirection::ToBottom.points(),
            (UnitPoint::new(0.5, 0.0), UnitPoint::new(0.5, 1.0))
        );
    }

    #[test]
    fn linear_defaults_to_bottom_and_follows_the_theme() {
        let gradient = linear(ColorToken::Primary, Color::WHITE).to_right();
        let resolved = BackgroundValue::from(gradient).resolve(&Tokens::light());
        let Background::Linear {
            from,
            to,
            start,
            end,
        } = resolved
        else {
            panic!("expected a linear background");
        };
        assert_eq!(from, Tokens::light().colors.primary);
        assert_eq!(to, Color::WHITE);
        assert_eq!(
            (start, end),
            (UnitPoint::new(0.0, 0.5), UnitPoint::new(1.0, 0.5))
        );
    }

    #[test]
    fn a_solid_color_compares_equal_to_that_color() {
        assert_eq!(Background::Solid(Color::BLACK), Color::BLACK);
        assert_ne!(
            Background::Linear {
                from: Color::BLACK,
                to: Color::WHITE,
                start: UnitPoint::new(0.0, 0.0),
                end: UnitPoint::new(1.0, 0.0),
            },
            Color::BLACK
        );
    }
}
