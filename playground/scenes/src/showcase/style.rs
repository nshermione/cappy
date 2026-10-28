//! The stylesheet: every class the layout may use. Values refer to theme tokens, so one
//! stylesheet serves every theme.

use cappy::prelude::*;

const HOVER: &str = ":hover";
const TILE_COLUMNS: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ClassName)]
pub enum Class {
    // Built into every cappy theme.
    App,
    Row,
    Column,
    Card,
    Title,
    Subtitle,
    Muted,
    Btn,
    BtnPrimary,
    Badge,
    Tile,
    // Defined by this stylesheet.
    Header,
    CardRow,
    Grow,
    Count,
    TileGrid,
    TileSelected,
    BadgeDone,
    TaskDone,
}

/// Adds the scene's own classes to `theme`.
pub fn stylesheet(theme: Theme) -> Theme {
    theme
        .with_class(
            Class::Header,
            style! {
                direction: FlexDirection::Row,
                align: Align::Center,
                justify: Justify::SpaceBetween,
            },
        )
        .with_class(
            Class::CardRow,
            style! {
                direction: FlexDirection::Row,
                align: Align::Stretch,
                gap: Space::Lg,
            },
        )
        .with_class(Class::Grow, style! { flex_grow: 1.0 })
        .with_class(Class::Count, style! { font_size: FontSize::Xl })
        .with_class(
            Class::TileGrid,
            style! {
                display: Display::Grid,
                grid_columns: TILE_COLUMNS,
                gap: Space::Sm,
            },
        )
        .with_class(
            Class::TileSelected,
            style! {
                background: ColorToken::Primary,
                border_color: ColorToken::Primary,
                text_color: ColorToken::OnPrimary,
            },
        )
        .with_class(
            hover(Class::TileSelected),
            style! { background: ColorToken::PrimaryHover },
        )
        .with_class(
            Class::BadgeDone,
            style! {
                background: ColorToken::Success,
                border_color: ColorToken::Success,
                text_color: ColorToken::OnPrimary,
            },
        )
        .with_class(
            Class::TaskDone,
            style! { text_color: ColorToken::TextMuted },
        )
}

fn hover(class: Class) -> String {
    format!("{class}{HOVER}")
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
