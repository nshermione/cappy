//! Themes and the stylesheet. A theme decides colors, radius, spacing and elevation.
//! The stylesheet is every class the layout may use. Values refer to theme tokens, so one
//! stylesheet serves every theme, and switching themes only repaints.

use cappy::prelude::*;

/// Near and far elevation, stored on a theme the first time it is overwritten.
#[style_token(f32)]
enum Depth {
    Near,
    Far,
}

/// The themes the header button cycles through, in order.
pub fn themes() -> [Theme; 3] {
    [Theme::light(), Theme::dark(), ocean()]
}

/// A custom theme: the dark one with blue surfaces, rounder corners and a deeper far elevation.
fn ocean() -> Theme {
    let mut theme = Theme::dark();
    theme.set_name("ocean");
    theme.patch_tokens((
        ColorToken::patch()
            .background(hex!("#0b1d2a"))
            .surface(hex!("#10293b"))
            .surface_alt(hex!("#15354c"))
            .surface_hover(hex!("#1c4462"))
            .border(hex!("#24506f"))
            .primary(hex!("#0ea5e9"))
            .primary_hover(hex!("#38bdf8"))
            .primary_active(hex!("#0284c7"))
            .on_primary(hex!("#04121c")),
        Radius::patch().md(10.0).lg(16.0),
    ));
    theme
}

const TILE_COLUMNS: u16 = 3;

stylesheet! {
    pub enum Class {
        Header {
            direction: FlexDirection::Row,
            align: Align::Center,
            justify: Justify::SpaceBetween,
        }
        TabBar {
            direction: FlexDirection::Row,
            align: Align::Center,
            gap: Space::Sm,
        }
        TabActive {
            background: ColorToken::Primary,
            border_color: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            :hover {
                background: ColorToken::PrimaryHover
            },
        }
        /// Takes leftover space in a fixed column. `min_height: 0` lets it shrink below its
        /// content, which is what gives a nested scroll column a definite height.
        Grow { flex_grow: 1.0, min_height: 0 }
        /// Fills leftover height and scrolls when its children are taller.
        /// `min_height: 0` lets the column shrink below its content so `overflow_y` can scroll.
        ScrollList {
            direction: FlexDirection::Column,
            flex_grow: 1.0,
            min_height: 0,
            gap: Space::Sm,
            overflow_y: Overflow::Auto,
        }
        Count { font_size: FontSize::Xl }
        TileGrid {
            display: Display::Grid,
            grid_columns: TILE_COLUMNS,
            gap: Space::Sm,
        }
        TileSelected {
            background: ColorToken::Primary,
            border_color: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            :hover {
                background: ColorToken::PrimaryHover
            },
        }
        BadgeDone {
            background: ColorToken::Success,
            border_color: ColorToken::Success,
            text_color: ColorToken::OnPrimary,
        }
        TaskDone { text_color: ColorToken::TextMuted }
        /// Shared size of the paint-tab specimens.
        Specimen {
            direction: FlexDirection::Column,
            align: Align::Center,
            gap: Space::Sm,
        }
        Swatch {
            width: 128,
            height: 72,
            radius: Depth::Far,
        }
        GradientRight {
            @include Swatch,
            background: linear(ColorToken::Primary, ColorToken::Success).to_right(),
        }
        GradientDown {
            @include Swatch,
            background: linear(ColorToken::Warning, ColorToken::Danger).to_bottom(),
        }
        GradientLeft {
            @include Swatch,
            background: linear(ColorToken::Primary, ColorToken::Warning).to_left(),
        }
        ShadowCard {
            width: 120,
            height: 72,
            radius: Radius::Lg,
            background: ColorToken::Surface,
            align: Align::Center,
            justify: Justify::Center,
        }
        ShadowSoft {
            @include ShadowCard,
            shadow: shadow(0, 8, 18, 0, Color::rgba(15, 23, 42, 90)),
        }
        ShadowHard {
            @include ShadowCard,
            shadow: shadow(6, 6, 0, 0, Color::rgba(15, 23, 42, 120)),
        }
        ShadowGlow {
            @include ShadowCard,
            shadow: shadow(0, 10, 22, 0, Color::rgba(14, 165, 233, 160)),
        }
        Picture {
            width: 112,
            height: 112,
            radius: Radius::Lg,
            background: ColorToken::SurfaceAlt,
        }
        /// A row of motion specimens. The gap and the boxes' vertical margin keep scale, spin
        /// and fly from painting over a neighbor.
        MotionRow {
            direction: FlexDirection::Row,
            align: Align::Center,
            gap: 64,
            padding_x: 24,
            flex_shrink: 0.0,
        }
        /// Ease tracks stacked so the motion scroll list can move down through them.
        MotionEases {
            direction: FlexDirection::Column,
            gap: Space::Md,
            flex_shrink: 0.0,
        }
        /// Colored box the motion tab fades with [`cappy::Motion::opacity`].
        MotionOpacity {
            width: 120,
            height: 72,
            radius: Radius::Lg,
            background: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            align: Align::Center,
            justify: Justify::Center,
            margin_y: 36,
        }
        /// Box the motion tab slides with [`cappy::Motion::translation`].
        MotionSlide {
            width: 120,
            height: 72,
            radius: Radius::Lg,
            background: ColorToken::Surface,
            border_color: ColorToken::Border,
            border_width: 1,
            align: Align::Center,
            justify: Justify::Center,
            margin_y: 36,
        }
        /// Fill the motion tab crossfades with [`cappy::Motion::background`].
        MotionColor {
            @include MotionOpacity,
        }
        /// Box the motion tab scales with [`cappy::Motion::scale`].
        MotionScale {
            @include MotionSlide,
            background: ColorToken::Success,
            border_width: 0,
            text_color: ColorToken::OnPrimary,
        }
        /// Wide bar the motion tab spins. A pip sits on the left so the turn is visible.
        MotionSpin {
            width: 104,
            height: 52,
            radius: Radius::Lg,
            background: ColorToken::Surface,
            border_color: ColorToken::Border,
            border_width: 1,
            direction: FlexDirection::Row,
            align: Align::Center,
            padding: Space::Sm,
            margin_y: 40,
        }
        MotionPip {
            width: 16,
            height: 16,
            radius: Radius::Sm,
            background: ColorToken::Primary,
        }
        /// Box the motion tab shakes or flies.
        MotionTravel {
            @include MotionSlide,
        }
        EaseName { width: 108 }
        EaseTrack {
            width: 188,
            height: 28,
            radius: Radius::Md,
            background: ColorToken::SurfaceAlt,
            direction: FlexDirection::Row,
            align: Align::Center,
        }
        EaseDot {
            width: 18,
            height: 18,
            radius: Radius::Md,
            background: ColorToken::Primary,
        }
    }
}

/// Adds the scene's own classes to `theme`. Light and dark themes receive the default elevation.
/// Ocean already stored its own.
pub fn stylesheet(mut theme: Theme) -> Theme {
    if !theme.has_tokens::<Depth>() {
        theme.patch_tokens(DepthPatch {
            near: Some(4.0),
            far: Some(12.0),
        });
    }
    Class::apply(theme)
}

/// The class list for a `:class` binding that adds `class` only while `active`.
pub fn class_when(active: bool, class: Class) -> String {
    if active {
        class.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Class, stylesheet, themes};

    #[test]
    fn every_class_is_defined_in_every_theme() {
        for theme in themes().map(stylesheet) {
            for class in Class::ALL {
                assert!(
                    theme.has_class(class.as_str()),
                    "theme {} lacks class {class}",
                    theme.name()
                );
            }
        }
    }
}
