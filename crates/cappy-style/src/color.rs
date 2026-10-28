use std::fmt;

/// An sRGB color with straight (non-premultiplied) alpha, each channel in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Reason a hex color string could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorParseError {
    MissingHash,
    InvalidLength(usize),
    InvalidDigit(u8),
}

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHash => write!(f, "hex color must start with '#'"),
            Self::InvalidLength(length) => write!(
                f,
                "hex color must have 3, 4, 6 or 8 digits after '#', found {length}"
            ),
            Self::InvalidDigit(byte) => {
                write!(f, "invalid hex digit {:?} in color", *byte as char)
            }
        }
    }
}

impl std::error::Error for ColorParseError {}

const CHANNEL_MAX: f32 = 255.0;

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / CHANNEL_MAX,
            g: g as f32 / CHANNEL_MAX,
            b: b as f32 / CHANNEL_MAX,
            a: a as f32 / CHANNEL_MAX,
        }
    }

    /// Parses `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    ///
    /// # Panics
    /// Panics on malformed input. Inside a `const` context (as the `hex!` macro does)
    /// the panic becomes a compile-time error. Use [`Color::parse_hex`] for runtime input.
    pub const fn hex(value: &str) -> Self {
        match Self::parse_hex(value) {
            Ok(color) => color,
            Err(_) => panic!("invalid hex color literal"),
        }
    }

    pub const fn parse_hex(value: &str) -> Result<Self, ColorParseError> {
        let bytes = value.as_bytes();
        if bytes.is_empty() || bytes[0] != b'#' {
            return Err(ColorParseError::MissingHash);
        }
        let digit_count = bytes.len() - 1;
        let mut nibbles = [0u8; 8];
        let mut index = 0;
        while index < digit_count {
            if index >= nibbles.len() {
                return Err(ColorParseError::InvalidLength(digit_count));
            }
            nibbles[index] = match hex_digit(bytes[index + 1]) {
                Ok(nibble) => nibble,
                Err(error) => return Err(error),
            };
            index += 1;
        }
        match digit_count {
            3 | 4 => {
                let alpha = if digit_count == 4 {
                    nibbles[3] * 17
                } else {
                    255
                };
                Ok(Self::rgba(
                    nibbles[0] * 17,
                    nibbles[1] * 17,
                    nibbles[2] * 17,
                    alpha,
                ))
            }
            6 | 8 => {
                let alpha = if digit_count == 8 {
                    nibbles[6] * 16 + nibbles[7]
                } else {
                    255
                };
                Ok(Self::rgba(
                    nibbles[0] * 16 + nibbles[1],
                    nibbles[2] * 16 + nibbles[3],
                    nibbles[4] * 16 + nibbles[5],
                    alpha,
                ))
            }
            other => Err(ColorParseError::InvalidLength(other)),
        }
    }

    pub const fn with_alpha(self, alpha: f32) -> Self {
        Self { a: alpha, ..self }
    }

    pub fn is_transparent(&self) -> bool {
        self.a <= 0.0
    }

    /// Converts to linear-light RGB with premultiplied alpha, the format GPU blending expects.
    pub fn to_linear_premultiplied(self) -> [f32; 4] {
        [
            srgb_to_linear(self.r) * self.a,
            srgb_to_linear(self.g) * self.a,
            srgb_to_linear(self.b) * self.a,
            self.a,
        ]
    }

    pub fn to_rgba8(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
            .map(|channel| (channel.clamp(0.0, 1.0) * CHANNEL_MAX).round() as u8)
    }
}

const fn hex_digit(byte: u8) -> Result<u8, ColorParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ColorParseError::InvalidDigit(byte)),
    }
}

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// Compile-time checked hex color: `hex!("#7c3aed")`.
#[macro_export]
macro_rules! hex {
    ($value:literal) => {
        const { $crate::Color::hex($value) }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!(Color::hex("#ff8000"), Color::rgb(255, 128, 0));
    }

    #[test]
    fn parses_short_and_alpha_forms() {
        assert_eq!(Color::hex("#fff"), Color::WHITE);
        assert_eq!(Color::hex("#0000"), Color::TRANSPARENT);
        assert_eq!(Color::hex("#00000080"), Color::rgba(0, 0, 0, 128));
    }

    #[test]
    fn rejects_malformed_hex() {
        assert_eq!(Color::parse_hex("fff"), Err(ColorParseError::MissingHash));
        assert_eq!(
            Color::parse_hex("#12345"),
            Err(ColorParseError::InvalidLength(5))
        );
        assert_eq!(
            Color::parse_hex("#12345678ab"),
            Err(ColorParseError::InvalidLength(10))
        );
        assert_eq!(
            Color::parse_hex("#ggg"),
            Err(ColorParseError::InvalidDigit(b'g'))
        );
    }

    #[test]
    fn hex_macro_evaluates_at_compile_time() {
        const PURPLE: Color = hex!("#7c3aed");
        assert_eq!(PURPLE.to_rgba8(), [0x7c, 0x3a, 0xed, 0xff]);
    }

    #[test]
    fn premultiplies_in_linear_space() {
        let [r, g, b, a] = Color::rgba(255, 255, 255, 128).to_linear_premultiplied();
        assert!((a - 128.0 / 255.0).abs() < 1e-6);
        assert!((r - a).abs() < 1e-6 && (g - a).abs() < 1e-6 && (b - a).abs() < 1e-6);
        let [mid, ..] = Color::rgb(128, 128, 128).to_linear_premultiplied();
        assert!((mid - 0.2158).abs() < 1e-3);
    }
}
