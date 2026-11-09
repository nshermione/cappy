//! Design tokens: named slots a theme stores and styles refer to.
//!
//! A `#[style_token]` enum names the slots. The macro also builds a `{Enum}Values` struct, which is
//! where the theme keeps the slot values. Built-in sets are fields of [`Tokens`]. A set defined
//! outside this crate is stored the first time [`Theme::overwrites`](crate::Theme::overwrites)
//! includes it.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

use cappy_macros::style_token;

use crate::Color;

/// A named slot in a token set.
pub trait StyleToken: Copy + 'static {
    type Value: Clone + PartialEq + Default + fmt::Debug + 'static;

    fn index(self) -> u8;
    fn from_index(index: u8) -> Self;
    fn read(self, tokens: &Tokens) -> Self::Value;
    fn stored(tokens: &Tokens) -> bool;
}

/// A token set whose values are stored on the theme when first overwritten, rather than in a
/// [`Tokens`] field.
#[doc(hidden)]
pub trait CustomToken: Copy + 'static {
    type Value: Clone + PartialEq + Default + fmt::Debug + 'static;
    type Values: Default + Clone + PartialEq + fmt::Debug + 'static;
    type Patch;

    fn read_values(self, values: &Self::Values) -> Self::Value;
    fn apply_patch(patch: Self::Patch, values: &mut Self::Values);
}

/// One patch, or a tuple of up to eight patches applied from left to right.
#[doc(hidden)]
pub trait PatchList {
    fn apply(self, tokens: &mut Tokens);
}

macro_rules! patch_list_tuples {
    ($($name:ident),+) => {
        impl<$($name: PatchList),+> PatchList for ($($name,)+) {
            fn apply(self, tokens: &mut Tokens) {
                #[allow(non_snake_case)]
                let ($($name,)+) = self;
                $($name.apply(tokens);)+
            }
        }
    };
}

patch_list_tuples!(A, B);
patch_list_tuples!(A, B, C);
patch_list_tuples!(A, B, C, D);
patch_list_tuples!(A, B, C, D, E);
patch_list_tuples!(A, B, C, D, E, F);
patch_list_tuples!(A, B, C, D, E, F, G);
patch_list_tuples!(A, B, C, D, E, F, G, H);

/// Names a color slot. Values live in [`ColorTokenValues`].
#[style_token(Color, colors)]
pub enum ColorToken {
    Background,
    Surface,
    SurfaceAlt,
    SurfaceHover,
    Border,
    Text,
    TextMuted,
    Primary,
    PrimaryHover,
    PrimaryActive,
    OnPrimary,
    Success,
    Warning,
    Danger,
}

/// Names a spacing slot, in logical pixels. Values live in [`SpaceValues`].
#[style_token(f32, spacing)]
pub enum Space {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
}

/// Names a corner-radius slot, in logical pixels. Values live in [`RadiusValues`].
#[style_token(f32, radius)]
pub enum Radius {
    Sm,
    Md,
    Lg,
    Full,
}

/// Names a font-size slot, in logical pixels. Values live in [`FontSizeValues`].
#[style_token(f32, font_sizes)]
pub enum FontSize {
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
}

/// Every built-in token set, plus any set defined with `#[style_token]` and overwritten onto a theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub colors: ColorTokenValues,
    pub spacing: SpaceValues,
    pub radius: RadiusValues,
    pub font_sizes: FontSizeValues,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    custom: HashMap<TypeId, ErasedValues>,
}

const FULLY_ROUNDED_RADIUS: f32 = 9999.0;

impl Tokens {
    /// Tokens with the given palette and the default spacing, radius and type scales.
    pub fn with_palette(colors: ColorTokenValues) -> Self {
        Self {
            colors,
            spacing: SpaceValues {
                xs: 4.0,
                sm: 8.0,
                md: 12.0,
                lg: 16.0,
                xl: 24.0,
            },
            radius: RadiusValues {
                sm: 4.0,
                md: 6.0,
                lg: 10.0,
                full: FULLY_ROUNDED_RADIUS,
            },
            font_sizes: FontSizeValues {
                sm: 12.0,
                md: 14.0,
                lg: 16.0,
                xl: 20.0,
                xxl: 28.0,
            },
            line_height: 1.4,
            custom: HashMap::new(),
        }
    }

    pub fn light() -> Self {
        Self::with_palette(ColorTokenValues {
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
        Self::with_palette(ColorTokenValues {
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

    pub fn overwrites<P: PatchList>(&mut self, patches: P) {
        patches.apply(self);
    }

    pub fn get<T: StyleToken>(&self, token: T) -> T::Value {
        token.read(self)
    }

    pub fn has_tokens<T: StyleToken>(&self) -> bool {
        T::stored(self)
    }

    #[doc(hidden)]
    pub fn apply_custom<T: CustomToken>(&mut self, patch: T::Patch) {
        let values = self.custom_mut::<T>();
        T::apply_patch(patch, values);
    }

    #[doc(hidden)]
    pub fn read_custom<T: CustomToken>(&self, token: T) -> T::Value {
        let Some(values) = self.custom_ref::<T>() else {
            panic!(
                "style token {} is not set on this theme",
                std::any::type_name::<T>()
            );
        };
        token.read_values(values)
    }

    #[doc(hidden)]
    pub fn has_custom<T: CustomToken>(&self) -> bool {
        self.custom_ref::<T>().is_some()
    }

    fn custom_mut<T: CustomToken>(&mut self) -> &mut T::Values {
        self.custom
            .entry(TypeId::of::<T>())
            .or_insert_with(|| ErasedValues::new(T::Values::default()))
            .downcast_mut()
    }

    fn custom_ref<T: CustomToken>(&self) -> Option<&T::Values> {
        self.custom
            .get(&TypeId::of::<T>())
            .and_then(ErasedValues::downcast_ref)
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Self::light()
    }
}

struct ErasedValues {
    data: Box<dyn AnyDebug>,
}

impl ErasedValues {
    fn new<T: AnyDebug>(value: T) -> Self {
        Self {
            data: Box::new(value),
        }
    }

    fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref()
    }

    fn downcast_mut<T: Any>(&mut self) -> &mut T {
        self.data
            .as_any_mut()
            .downcast_mut()
            .expect("custom style token slot has the wrong type")
    }
}

impl Clone for ErasedValues {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone_box(),
        }
    }
}

impl PartialEq for ErasedValues {
    fn eq(&self, other: &Self) -> bool {
        self.data.eq_box(other.data.as_ref())
    }
}

impl fmt::Debug for ErasedValues {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.data.fmt_box(formatter)
    }
}

trait AnyDebug: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn eq_box(&self, other: &dyn AnyDebug) -> bool;
    fn clone_box(&self) -> Box<dyn AnyDebug>;
    fn fmt_box(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;
}

impl<T> AnyDebug for T
where
    T: Clone + PartialEq + fmt::Debug + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn eq_box(&self, other: &dyn AnyDebug) -> bool {
        other
            .as_any()
            .downcast_ref::<T>()
            .is_some_and(|other| self == other)
    }

    fn clone_box(&self) -> Box<dyn AnyDebug> {
        Box::new(self.clone())
    }

    fn fmt_box(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;

    #[test]
    fn token_lookup_returns_slot_value() {
        let tokens = Tokens::light();
        assert_eq!(
            tokens.colors.get(ColorToken::Primary),
            tokens.colors.primary
        );
        assert_eq!(tokens.get(ColorToken::Primary), tokens.colors.primary);
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

    #[test]
    fn patch_changes_only_the_slots_it_sets() {
        let mut theme = Theme::light();
        let text = theme.tokens().colors.text;
        theme.patch_tokens((
            ColorToken::patch().primary(Color::hex("#112233")),
            Radius::patch().md(10.0),
        ));
        assert_eq!(theme.tokens().colors.primary, Color::hex("#112233"));
        assert_eq!(theme.tokens().colors.text, text);
        assert_eq!(theme.tokens().radius.md, 10.0);
        assert_eq!(theme.tokens().radius.lg, Tokens::light().radius.lg);
        assert!(theme.has_tokens::<ColorToken>());
    }
}
