//! Built-in classes. One definition produces the [`Class`] enum and the rules [`Theme::light`]
//! and [`Theme::dark`] share.

use cappy_macros::stylesheet;

use crate::{
    Align, ColorToken, FlexDirection, FontSize, FontWeight, Justify, Radius, Space,
    StyleRefinement, Stylesheet, Theme, Tokens,
};

stylesheet! {
    pub enum Class {
        App {
            direction: FlexDirection::Column,
            size: crate::percent(100.0),
            padding: Space::Xl,
            gap: Space::Lg,
            background: ColorToken::Background,
            text_color: ColorToken::Text,
            font_size: FontSize::Md,
        }
        Row {
            direction: FlexDirection::Row,
            align: Align::Center,
            gap: Space::Sm,
        }
        Column {
            direction: FlexDirection::Column,
            gap: Space::Sm,
        }
        Card {
            direction: FlexDirection::Column,
            padding: Space::Lg,
            gap: Space::Md,
            radius: Radius::Lg,
            border_width: 1,
            border_color: ColorToken::Border,
            background: ColorToken::Surface,
        }
        Title {
            font_size: FontSize::Xxl,
            font_weight: FontWeight::BOLD,
        }
        Subtitle {
            font_size: FontSize::Lg,
            font_weight: FontWeight::SEMIBOLD,
        }
        Muted {
            text_color: ColorToken::TextMuted,
            font_size: FontSize::Sm,
        }
        Btn {
            direction: FlexDirection::Row,
            align: Align::Center,
            justify: Justify::Center,
            padding_x: Space::Lg,
            padding_y: Space::Sm,
            radius: Radius::Md,
            border_width: 1,
            border_color: ColorToken::Border,
            background: ColorToken::Surface,
            font_weight: FontWeight::MEDIUM,
            :hover { background: ColorToken::SurfaceHover },
            :active { border_color: ColorToken::Primary },
        }
        BtnPrimary {
            background: ColorToken::Primary,
            border_color: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            :hover { background: ColorToken::PrimaryHover },
            :active { background: ColorToken::PrimaryActive },
        }
        Badge {
            padding_x: Space::Sm,
            padding_y: 2,
            radius: Radius::Full,
            background: ColorToken::SurfaceAlt,
            border_width: 1,
            border_color: ColorToken::Border,
            font_size: FontSize::Sm,
        }
        Tile {
            padding: Space::Md,
            radius: Radius::Md,
            background: ColorToken::SurfaceAlt,
            border_width: 1,
            border_color: ColorToken::Border,
            :hover { background: ColorToken::SurfaceHover },
        }
    }
}

impl Theme {
    /// Light theme with the built-in class set.
    pub fn light() -> Self {
        Class::apply(Self::new("light", Tokens::light()))
    }

    /// Dark theme with the built-in class set. Same classes as [`Theme::light`], other tokens.
    pub fn dark() -> Self {
        Class::apply(Self::new("dark", Tokens::dark()))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use cappy_macros::stylesheet;

    use crate::{
        ClassName, Color, ColorToken, Edges, InteractionState, Space, StyleRefinement, Stylesheet,
        Theme, Tokens,
    };

    stylesheet! {
        enum Sample {
            Later {
                text_color: ColorToken::TextMuted,
            }
            Earlier {
                @include Later,
                background: ColorToken::Surface,
            }
            Badge {
                background: ColorToken::Surface,
                text_color: ColorToken::Text,
                :active { background: ColorToken::PrimaryActive },
            }
            BadgeDone {
                @include Badge,
                background: ColorToken::Success,
            }
            Tile {
                background: ColorToken::SurfaceAlt,
                padding: Space::Md,
                :hover { background: ColorToken::SurfaceHover },
            }
        }
    }

    fn names(list: &str) -> Vec<ClassName> {
        ClassName::parse_list(list)
    }

    fn paint(theme: &Theme, list: &str, state: InteractionState) -> (crate::Background, Color) {
        let style = theme.resolve(None, &names(list), &StyleRefinement::default(), state, None);
        (style.visual.background, style.text.color)
    }

    #[test]
    fn include_copies_properties_and_later_ones_win() {
        let theme = Sample::apply(Theme::new("plain", Tokens::light()));
        let colors = theme.tokens().colors.clone();
        let (background, text) = paint(&theme, "badge-done", InteractionState::default());
        assert_eq!(background, colors.success);
        assert_eq!(text, colors.text);
        let (background, text) = paint(&theme, "earlier", InteractionState::default());
        assert_eq!(background, colors.surface);
        assert_eq!(text, colors.text_muted);
        assert!(theme.has_class("later"));
        assert!(!Theme::new("plain", Tokens::light()).has_class("badge-done"));
    }

    #[test]
    fn hover_and_active_stay_on_the_named_class() {
        let theme = Sample::apply(Theme::new("plain", Tokens::light()));
        let colors = theme.tokens().colors.clone();
        let (rest, _) = paint(&theme, "tile", InteractionState::default());
        let (hovered, _) = paint(
            &theme,
            "tile",
            InteractionState {
                hovered: true,
                pressed: false,
                focused: false,
                disabled: false,
            },
        );
        let hovered_style = theme.resolve(
            None,
            &names("tile"),
            &StyleRefinement::default(),
            InteractionState {
                hovered: true,
                pressed: false,
                focused: false,
                disabled: false,
            },
            None,
        );
        let (pressed, pressed_text) = paint(
            &theme,
            "badge",
            InteractionState {
                hovered: false,
                pressed: true,
                focused: false,
                disabled: false,
            },
        );
        assert_eq!(rest, colors.surface_alt);
        assert_eq!(hovered, colors.surface_hover);
        assert_eq!(
            hovered_style.layout.padding,
            Edges::all(theme.tokens().spacing.md)
        );
        assert_eq!(pressed, colors.primary_active);
        assert_eq!(pressed_text, colors.text);
        assert_eq!(Sample::ALL.len(), 5);
        assert_eq!(Sample::Tile.as_str(), "tile");
    }
}
