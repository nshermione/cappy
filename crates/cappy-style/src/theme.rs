//! Themes: tokens plus named style classes. Swapping the theme restyles a UI without
//! touching its layout code.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::{
    Align, ColorToken, FlexDirection, FontSize, FontWeight, Justify, Radius, Space, Style,
    StyleRefinement, TextStyle, Tokens,
};

/// Name of a style class, e.g. `"card"` or `"btn-primary"`.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ClassName(Arc<str>);

impl ClassName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Splits a space-separated class list such as `"btn btn-primary"`.
    pub fn parse_list(list: &str) -> Vec<ClassName> {
        list.split_whitespace().map(ClassName::from).collect()
    }
}

impl fmt::Debug for ClassName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl From<&str> for ClassName {
    fn from(name: &str) -> Self {
        Self(Arc::from(name))
    }
}

impl From<String> for ClassName {
    fn from(name: String) -> Self {
        Self(Arc::from(name))
    }
}

/// Interaction state used to pick `:hover` / `:active` class variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InteractionState {
    pub hovered: bool,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClassVariant {
    Base,
    Hover,
    Active,
}

const HOVER_SUFFIX: &str = ":hover";
const ACTIVE_SUFFIX: &str = ":active";

impl ClassVariant {
    fn split(selector: &str) -> (&str, Self) {
        if let Some(name) = selector.strip_suffix(HOVER_SUFFIX) {
            (name, Self::Hover)
        } else if let Some(name) = selector.strip_suffix(ACTIVE_SUFFIX) {
            (name, Self::Active)
        } else {
            (selector, Self::Base)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ClassRule {
    base: Option<StyleRefinement>,
    hover: Option<StyleRefinement>,
    active: Option<StyleRefinement>,
}

impl ClassRule {
    fn slot_mut(&mut self, variant: ClassVariant) -> &mut Option<StyleRefinement> {
        match variant {
            ClassVariant::Base => &mut self.base,
            ClassVariant::Hover => &mut self.hover,
            ClassVariant::Active => &mut self.active,
        }
    }
}

/// Tokens plus a set of named classes.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    name: String,
    tokens: Tokens,
    classes: HashMap<String, ClassRule>,
}

impl Theme {
    pub fn new(name: impl Into<String>, tokens: Tokens) -> Self {
        Self {
            name: name.into(),
            tokens,
            classes: HashMap::new(),
        }
    }

    /// Light theme with the built-in class set.
    pub fn light() -> Self {
        Self::new("light", Tokens::light()).with_default_classes()
    }

    /// Dark theme with the built-in class set. Same classes as [`Theme::light`], other tokens.
    pub fn dark() -> Self {
        Self::new("dark", Tokens::dark()).with_default_classes()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    pub fn tokens_mut(&mut self) -> &mut Tokens {
        &mut self.tokens
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Defines or replaces a class. `selector` may end in `:hover` or `:active`.
    pub fn set_class(&mut self, selector: impl AsRef<str>, refinement: StyleRefinement) {
        let (name, variant) = ClassVariant::split(selector.as_ref());
        *self
            .classes
            .entry(name.to_owned())
            .or_default()
            .slot_mut(variant) = Some(refinement);
    }

    #[must_use]
    pub fn with_class(mut self, selector: impl AsRef<str>, refinement: StyleRefinement) -> Self {
        self.set_class(selector, refinement);
        self
    }

    pub fn has_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    /// Resolves the style of a node.
    ///
    /// Layers, lowest first: text inherited from `parent_text` (or the theme defaults for the
    /// root), each class in order, each class's `:hover` then `:active` variant when the
    /// state matches, then `inline`.
    pub fn resolve(
        &self,
        parent_text: Option<&TextStyle>,
        classes: &[ClassName],
        inline: &StyleRefinement,
        state: InteractionState,
    ) -> Style {
        let inherited = parent_text
            .cloned()
            .unwrap_or_else(|| TextStyle::from_tokens(&self.tokens));
        let mut style = Style::inheriting(inherited);
        let rules: Vec<&ClassRule> = classes
            .iter()
            .filter_map(|class| self.classes.get(class.as_str()))
            .collect();
        let apply = |style: &mut Style, layer: &Option<StyleRefinement>| {
            if let Some(refinement) = layer {
                refinement.apply_to(style, &self.tokens);
            }
        };
        for rule in &rules {
            apply(&mut style, &rule.base);
        }
        if state.hovered {
            for rule in &rules {
                apply(&mut style, &rule.hover);
            }
        }
        if state.pressed {
            for rule in &rules {
                apply(&mut style, &rule.active);
            }
        }
        inline.apply_to(&mut style, &self.tokens);
        style
    }

    fn with_default_classes(self) -> Self {
        let style = StyleRefinement::default;
        let button = style()
            .direction(FlexDirection::Row)
            .align(Align::Center)
            .justify(Justify::Center)
            .padding_x(Space::Lg)
            .padding_y(Space::Sm)
            .radius(Radius::Md)
            .border_width(1)
            .border_color(ColorToken::Border)
            .background(ColorToken::Surface)
            .font_weight(FontWeight::MEDIUM);
        self.with_class(
            "app",
            style()
                .direction(FlexDirection::Column)
                .size(crate::percent(100.0))
                .padding(Space::Xl)
                .gap(Space::Lg)
                .background(ColorToken::Background)
                .text_color(ColorToken::Text)
                .font_size(FontSize::Md),
        )
        .with_class(
            "row",
            style()
                .direction(FlexDirection::Row)
                .align(Align::Center)
                .gap(Space::Sm),
        )
        .with_class(
            "column",
            style().direction(FlexDirection::Column).gap(Space::Sm),
        )
        .with_class(
            "card",
            style()
                .direction(FlexDirection::Column)
                .padding(Space::Lg)
                .gap(Space::Md)
                .radius(Radius::Lg)
                .border_width(1)
                .border_color(ColorToken::Border)
                .background(ColorToken::Surface),
        )
        .with_class(
            "title",
            style()
                .font_size(FontSize::Xxl)
                .font_weight(FontWeight::BOLD),
        )
        .with_class(
            "subtitle",
            style()
                .font_size(FontSize::Lg)
                .font_weight(FontWeight::SEMIBOLD),
        )
        .with_class(
            "muted",
            style()
                .text_color(ColorToken::TextMuted)
                .font_size(FontSize::Sm),
        )
        .with_class("btn", button)
        .with_class("btn:hover", style().background(ColorToken::SurfaceHover))
        .with_class("btn:active", style().border_color(ColorToken::Primary))
        .with_class(
            "btn-primary",
            style()
                .background(ColorToken::Primary)
                .border_color(ColorToken::Primary)
                .text_color(ColorToken::OnPrimary),
        )
        .with_class(
            "btn-primary:hover",
            style().background(ColorToken::PrimaryHover),
        )
        .with_class(
            "btn-primary:active",
            style().background(ColorToken::PrimaryActive),
        )
        .with_class(
            "badge",
            style()
                .padding_x(Space::Sm)
                .padding_y(2)
                .radius(Radius::Full)
                .background(ColorToken::SurfaceAlt)
                .border_width(1)
                .border_color(ColorToken::Border)
                .font_size(FontSize::Sm),
        )
        .with_class(
            "tile",
            style()
                .padding(Space::Md)
                .radius(Radius::Md)
                .background(ColorToken::SurfaceAlt)
                .border_width(1)
                .border_color(ColorToken::Border),
        )
        .with_class("tile:hover", style().background(ColorToken::SurfaceHover))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Edges};

    fn classes(list: &str) -> Vec<ClassName> {
        ClassName::parse_list(list)
    }

    #[test]
    fn parses_space_separated_class_lists() {
        let names: Vec<String> = classes("  btn  btn-primary ")
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        assert_eq!(names, ["btn", "btn-primary"]);
    }

    #[test]
    fn later_classes_override_earlier_ones() {
        let theme = Theme::light();
        let style = theme.resolve(
            None,
            &classes("btn btn-primary"),
            &StyleRefinement::default(),
            InteractionState::default(),
        );
        assert_eq!(style.visual.background, theme.tokens().colors.primary);
        assert_eq!(style.text.color, theme.tokens().colors.on_primary);
    }

    #[test]
    fn hover_and_active_variants_apply_by_state() {
        let theme = Theme::light();
        let colors = &theme.tokens().colors;
        let resolve = |hovered, pressed| {
            theme
                .resolve(
                    None,
                    &classes("btn btn-primary"),
                    &StyleRefinement::default(),
                    InteractionState { hovered, pressed },
                )
                .visual
                .background
        };
        assert_eq!(resolve(false, false), colors.primary);
        assert_eq!(resolve(true, false), colors.primary_hover);
        assert_eq!(resolve(true, true), colors.primary_active);
    }

    #[test]
    fn inline_style_wins_over_classes() {
        let theme = Theme::light();
        let style = theme.resolve(
            None,
            &classes("card"),
            &StyleRefinement::default()
                .padding(2)
                .background(Color::BLACK),
            InteractionState::default(),
        );
        assert_eq!(style.layout.padding, Edges::all(2.0));
        assert_eq!(style.visual.background, Color::BLACK);
    }

    #[test]
    fn text_is_inherited_from_parent_and_layout_is_not() {
        let theme = Theme::light();
        let parent = theme.resolve(
            None,
            &classes("card title"),
            &StyleRefinement::default(),
            InteractionState::default(),
        );
        let child = theme.resolve(
            Some(&parent.text),
            &[],
            &StyleRefinement::default(),
            InteractionState::default(),
        );
        assert_eq!(child.text, parent.text);
        assert_eq!(child.layout.padding, Edges::ZERO);
        assert_eq!(child.visual.background, Color::TRANSPARENT);
    }

    #[test]
    fn switching_theme_changes_paint_but_not_layout() {
        let resolve = |theme: &Theme| {
            theme.resolve(
                None,
                &classes("card"),
                &StyleRefinement::default(),
                InteractionState::default(),
            )
        };
        let light = resolve(&Theme::light());
        let dark = resolve(&Theme::dark());
        let change = dark.diff(&light);
        assert!(change.paint && !change.layout);
    }

    #[test]
    fn unknown_classes_are_ignored() {
        let theme = Theme::light();
        let style = theme.resolve(
            None,
            &classes("does-not-exist"),
            &StyleRefinement::default(),
            InteractionState::default(),
        );
        assert_eq!(style.visual.background, Color::TRANSPARENT);
        assert!(!theme.has_class("does-not-exist"));
    }
}
