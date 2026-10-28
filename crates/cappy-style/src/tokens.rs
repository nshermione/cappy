//! Design tokens: the named values a theme provides and styles refer to.
//!
//! Each token set is a struct of concrete values plus an enum naming its slots, so styles can
//! reference a slot (`ColorToken::Primary`) and stay valid when the theme swaps the values.

use crate::Color;

macro_rules! token_set {
    (
        $(#[$struct_meta:meta])*
        struct $set:ident : $value:ty;
        $(#[$enum_meta:meta])*
        enum $token:ident { $($field:ident => $variant:ident),+ $(,)? }
    ) => {
        $(#[$struct_meta])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $set {
            $(pub $field: $value,)+
        }

        $(#[$enum_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $token {
            $($variant,)+
        }

        impl $set {
            pub fn get(&self, token: $token) -> $value {
                match token {
                    $($token::$variant => self.$field,)+
                }
            }
        }
    };
}

token_set! {
    /// Semantic color palette.
    struct Palette: Color;
    /// Names a [`Palette`] slot.
    enum ColorToken {
        background => Background,
        surface => Surface,
        surface_alt => SurfaceAlt,
        surface_hover => SurfaceHover,
        border => Border,
        text => Text,
        text_muted => TextMuted,
        primary => Primary,
        primary_hover => PrimaryHover,
        primary_active => PrimaryActive,
        on_primary => OnPrimary,
        success => Success,
        warning => Warning,
        danger => Danger,
    }
}

token_set! {
    /// Spacing scale in logical pixels, used for padding, margin and gap.
    struct SpacingScale: f32;
    /// Names a [`SpacingScale`] step.
    enum Space { xs => Xs, sm => Sm, md => Md, lg => Lg, xl => Xl }
}

token_set! {
    /// Corner radius scale in logical pixels.
    struct RadiusScale: f32;
    /// Names a [`RadiusScale`] step.
    enum Radius { sm => Sm, md => Md, lg => Lg, full => Full }
}

token_set! {
    /// Font size scale in logical pixels.
    struct FontSizeScale: f32;
    /// Names a [`FontSizeScale`] step.
    enum FontSize { sm => Sm, md => Md, lg => Lg, xl => Xl, xxl => Xxl }
}

/// Every token a theme provides.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub colors: Palette,
    pub spacing: SpacingScale,
    pub radius: RadiusScale,
    pub font_sizes: FontSizeScale,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
}

const FULLY_ROUNDED_RADIUS: f32 = 9999.0;

impl Tokens {
    /// Tokens with the given palette and the default spacing, radius and type scales.
    pub fn with_palette(colors: Palette) -> Self {
        Self {
            colors,
            spacing: SpacingScale {
                xs: 4.0,
                sm: 8.0,
                md: 12.0,
                lg: 16.0,
                xl: 24.0,
            },
            radius: RadiusScale {
                sm: 4.0,
                md: 6.0,
                lg: 10.0,
                full: FULLY_ROUNDED_RADIUS,
            },
            font_sizes: FontSizeScale {
                sm: 12.0,
                md: 14.0,
                lg: 16.0,
                xl: 20.0,
                xxl: 28.0,
            },
            line_height: 1.4,
        }
    }

    pub fn light() -> Self {
        Self::with_palette(Palette {
            background: Color::hex("#f4f4f5"),
            surface: Color::hex("#ffffff"),
            surface_alt: Color::hex("#f8fafc"),
            surface_hover: Color::hex("#eef2f7"),
            border: Color::hex("#d4d4d8"),
            text: Color::hex("#18181b"),
            text_muted: Color::hex("#71717a"),
            primary: Color::hex("#10b981"),
            primary_hover: Color::hex("#059669"),
            primary_active: Color::hex("#047857"),
            on_primary: Color::hex("#ffffff"),
            success: Color::hex("#22c55e"),
            warning: Color::hex("#f59e0b"),
            danger: Color::hex("#ef4444"),
        })
    }

    pub fn dark() -> Self {
        Self::with_palette(Palette {
            background: Color::hex("#09090b"),
            surface: Color::hex("#18181b"),
            surface_alt: Color::hex("#1f1f23"),
            surface_hover: Color::hex("#27272a"),
            border: Color::hex("#3f3f46"),
            text: Color::hex("#fafafa"),
            text_muted: Color::hex("#a1a1aa"),
            primary: Color::hex("#34d399"),
            primary_hover: Color::hex("#6ee7b7"),
            primary_active: Color::hex("#a7f3d0"),
            on_primary: Color::hex("#022c22"),
            success: Color::hex("#4ade80"),
            warning: Color::hex("#fbbf24"),
            danger: Color::hex("#f87171"),
        })
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_lookup_returns_slot_value() {
        let tokens = Tokens::light();
        assert_eq!(
            tokens.colors.get(ColorToken::Primary),
            tokens.colors.primary
        );
        assert_eq!(tokens.spacing.get(Space::Md), 12.0);
        assert_eq!(tokens.radius.get(Radius::Full), FULLY_ROUNDED_RADIUS);
        assert_eq!(tokens.font_sizes.get(FontSize::Xl), 20.0);
    }

    #[test]
    fn light_and_dark_share_scales_but_not_colors() {
        let light = Tokens::light();
        let dark = Tokens::dark();
        assert_eq!(light.spacing, dark.spacing);
        assert_ne!(light.colors.background, dark.colors.background);
    }
}
