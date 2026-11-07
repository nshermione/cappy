//! The stylesheet: every class the layout may use. Values refer to theme tokens, so one
//! stylesheet serves every theme.

use cappy::prelude::*;

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
        }
        Grow { flex_grow: 1.0 }
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
            radius: Radius::Md,
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
    }
    TileSelected:hover { background: ColorToken::PrimaryHover }
    TabActive:hover { background: ColorToken::PrimaryHover }
}

/// Adds the scene's own classes to `theme`.
pub fn stylesheet(theme: Theme) -> Theme {
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
    use super::{Class, stylesheet};
    use crate::showcase::theme::themes;

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
