//! A single drop shadow. Painting may extend outside the border box.

use crate::{Color, ColorValue, Length, Tokens};

/// Standard deviation of the blur, as a fraction of the CSS blur radius.
const SIGMA_FACTOR: f32 = 0.5;
/// How many standard deviations of the blur are painted and damaged.
pub const BLUR_EXTENT_SIGMAS: f32 = 3.0;

/// A resolved drop shadow. A transparent color means the node casts no shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

impl Default for BoxShadow {
    fn default() -> Self {
        Self::NONE
    }
}

impl BoxShadow {
    pub const NONE: Self = Self {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 0.0,
        spread: 0.0,
        color: Color::TRANSPARENT,
    };

    pub fn is_visible(self) -> bool {
        !self.color.is_transparent()
    }

    /// How far the blur is painted past the shadow shape, in the same pixels as `blur`.
    pub fn blur_extent(self) -> f32 {
        self.blur.max(0.0) * SIGMA_FACTOR * BLUR_EXTENT_SIGMAS
    }

    /// Coverage of a blurred rounded box at `signed_distance` (negative inside the shape).
    ///
    /// `blur` is the CSS blur radius. Zero blur is a one-pixel antialiased edge. A positive blur
    /// is a Gaussian with standard deviation `blur / 2`, matching the CSS shadow filter.
    pub fn coverage(signed_distance: f32, blur: f32) -> f32 {
        if blur <= 0.0 {
            return (0.5 - signed_distance).clamp(0.0, 1.0);
        }
        let sigma = blur * SIGMA_FACTOR;
        let x = signed_distance / (sigma * std::f32::consts::SQRT_2);
        0.5 * erfc_approx(x)
    }
}

/// A drop shadow whose lengths and color may still be theme tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowValue {
    pub offset_x: Length,
    pub offset_y: Length,
    pub blur: Length,
    pub spread: Length,
    pub color: ColorValue,
}

impl ShadowValue {
    pub fn resolve(self, tokens: &Tokens) -> BoxShadow {
        BoxShadow {
            offset_x: self.offset_x.resolve(tokens),
            offset_y: self.offset_y.resolve(tokens),
            blur: self.blur.resolve(tokens),
            spread: self.spread.resolve(tokens),
            color: self.color.resolve(tokens),
        }
    }
}

/// A drop shadow of `color`, offset by `(offset_x, offset_y)`, blurred and spread.
pub fn shadow(
    offset_x: impl Into<Length>,
    offset_y: impl Into<Length>,
    blur: impl Into<Length>,
    spread: impl Into<Length>,
    color: impl Into<ColorValue>,
) -> ShadowValue {
    ShadowValue {
        offset_x: offset_x.into(),
        offset_y: offset_y.into(),
        blur: blur.into(),
        spread: spread.into(),
        color: color.into(),
    }
}

/// Abramowitz and Stegun 7.1.26, accurate to about `1.5e-7`.
/// The coefficients are the published ones; shortening them would drift from the GPU shader.
#[allow(clippy::excessive_precision)]
fn erf_approx(x: f32) -> f32 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let polynomial = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736)
        * t
        + 0.254829592)
        * t;
    sign * (1.0 - polynomial * (-x * x).exp())
}

fn erfc_approx(x: f32) -> f32 {
    1.0 - erf_approx(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorToken, Tokens};

    #[test]
    fn a_transparent_shadow_is_not_visible() {
        assert!(!BoxShadow::NONE.is_visible());
    }

    #[test]
    fn blur_extent_is_three_standard_deviations() {
        let shadow = shadow(0, 0, 20, 0, Color::BLACK).resolve(&Tokens::light());
        assert!((shadow.blur_extent() - 30.0).abs() < 1e-4);
    }

    #[test]
    fn coverage_is_opaque_inside_and_half_on_a_blurred_edge() {
        assert_eq!(BoxShadow::coverage(-10.0, 0.0), 1.0);
        assert_eq!(BoxShadow::coverage(10.0, 0.0), 0.0);
        let edge = BoxShadow::coverage(0.0, 8.0);
        assert!((edge - 0.5).abs() < 1e-4, "edge coverage was {edge}");
        assert!(BoxShadow::coverage(8.0 * SIGMA_FACTOR * BLUR_EXTENT_SIGMAS, 8.0) < 0.01);
    }

    #[test]
    fn lengths_and_tokens_resolve() {
        let resolved = shadow(4, 0, 2, 1, ColorToken::Text).resolve(&Tokens::light());
        assert_eq!(resolved.offset_x, 4.0);
        assert_eq!(resolved.blur, 2.0);
        assert_eq!(resolved.spread, 1.0);
        assert_eq!(resolved.color, Tokens::light().colors.text);
    }
}
