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
        CardRow {
            direction: FlexDirection::Row,
            align: Align::Stretch,
            gap: Space::Lg,
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
    }
    TileSelected:hover { background: ColorToken::PrimaryHover }
}

/// Adds the scene's own classes to `theme`.
pub fn stylesheet(theme: Theme) -> Theme {
    Class::apply(theme)
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
