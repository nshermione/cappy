//! Themes: tokens plus named style classes. Swapping the theme restyles a UI without
//! touching its layout code.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::{PatchList, Style, StyleRefinement, StyleToken, TextStyle, Tokens};

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

/// Interaction state used to pick `:hover`, `:focus`, `:active` and `:disabled` class variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InteractionState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClassVariant {
    Base,
    Hover,
    Focus,
    Active,
    Disabled,
}

const HOVER_SUFFIX: &str = ":hover";
const FOCUS_SUFFIX: &str = ":focus";
const ACTIVE_SUFFIX: &str = ":active";
const DISABLED_SUFFIX: &str = ":disabled";

impl ClassVariant {
    fn split(selector: &str) -> (&str, Self) {
        if let Some(name) = selector.strip_suffix(HOVER_SUFFIX) {
            (name, Self::Hover)
        } else if let Some(name) = selector.strip_suffix(FOCUS_SUFFIX) {
            (name, Self::Focus)
        } else if let Some(name) = selector.strip_suffix(ACTIVE_SUFFIX) {
            (name, Self::Active)
        } else if let Some(name) = selector.strip_suffix(DISABLED_SUFFIX) {
            (name, Self::Disabled)
        } else {
            (selector, Self::Base)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ClassRule {
    base: Option<StyleRefinement>,
    hover: Option<StyleRefinement>,
    focus: Option<StyleRefinement>,
    active: Option<StyleRefinement>,
    disabled: Option<StyleRefinement>,
}

impl ClassRule {
    fn slot_mut(&mut self, variant: ClassVariant) -> &mut Option<StyleRefinement> {
        match variant {
            ClassVariant::Base => &mut self.base,
            ClassVariant::Hover => &mut self.hover,
            ClassVariant::Focus => &mut self.focus,
            ClassVariant::Active => &mut self.active,
            ClassVariant::Disabled => &mut self.disabled,
        }
    }
}

/// Named classes with `:hover`, `:focus`, `:active` and `:disabled` variants. A [`Theme`] shares one of these with every
/// node. A component may carry its own. That sheet is stamped onto the rendered tree. A class it
/// does not define falls through to the theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stylesheet {
    classes: HashMap<String, ClassRule>,
}

impl Stylesheet {
    /// Defines or replaces a class. `selector` may end in `:hover`, `:focus`, `:active` or `:disabled`.
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

    /// Copies every variant `other` sets. Variants `other` leaves unset stay as they are.
    pub fn merge(&mut self, other: &Self) {
        for (name, rule) in &other.classes {
            let slot = self.classes.entry(name.clone()).or_default();
            if rule.base.is_some() {
                slot.base = rule.base.clone();
            }
            if rule.hover.is_some() {
                slot.hover = rule.hover.clone();
            }
            if rule.focus.is_some() {
                slot.focus = rule.focus.clone();
            }
            if rule.active.is_some() {
                slot.active = rule.active.clone();
            }
            if rule.disabled.is_some() {
                slot.disabled = rule.disabled.clone();
            }
        }
    }

    fn rule(&self, name: &str) -> Option<&ClassRule> {
        self.classes.get(name)
    }
}

/// Tokens plus a set of named classes.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    name: String,
    tokens: Tokens,
    classes: Stylesheet,
}

impl Theme {
    pub fn new(name: impl Into<String>, tokens: Tokens) -> Self {
        Self {
            name: name.into(),
            tokens,
            classes: Stylesheet::default(),
        }
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

    /// Defines or replaces a class. `selector` may end in `:hover`, `:focus`, `:active` or `:disabled`.
    pub fn set_class(&mut self, selector: impl AsRef<str>, refinement: StyleRefinement) {
        self.classes.set_class(selector, refinement);
    }

    #[must_use]
    pub fn with_class(mut self, selector: impl AsRef<str>, refinement: StyleRefinement) -> Self {
        self.set_class(selector, refinement);
        self
    }

    /// Merges `sheet` into this theme. Every node can then use those classes.
    #[must_use]
    pub fn with_sheet(mut self, sheet: &Stylesheet) -> Self {
        self.classes.merge(sheet);
        self
    }

    pub fn has_class(&self, name: &str) -> bool {
        self.classes.has_class(name)
    }

    /// Applies one patch, or a tuple of up to eight patches, from left to right.
    ///
    /// A setter on a patch changes that slot. A slot with no setter keeps the value already on
    /// this theme. A token set that is not one of the built-in fields is stored on the first call.
    pub fn patch_tokens<P: PatchList>(&mut self, patches: P) {
        self.tokens.overwrites(patches);
    }

    /// The value stored for `token`.
    ///
    /// # Panics
    /// Panics when `token` belongs to a set that has never been [overwritten](Self::overwrites)
    /// onto this theme. Built-in tokens are always present.
    pub fn get<T: StyleToken>(&self, token: T) -> T::Value {
        self.tokens.get(token)
    }

    /// Whether this theme has values for `token`'s set.
    pub fn has_tokens<T: StyleToken>(&self) -> bool {
        self.tokens.has_tokens::<T>()
    }

    /// Resolves the style of a node.
    ///
    /// Layers, lowest first: text inherited from `parent_text` (or the theme defaults for the
    /// root), each class in order, each class's `:disabled` variant when the node is disabled,
    /// otherwise `:hover`, then `:focus`, then `:active` when the state matches, then `inline`.
    /// A name present in `local` uses that sheet's rule, including its interaction variants.
    /// Names `local` does not define fall through
    /// to the theme.
    /// `local` is the sheet of this node only; child nodes do not receive it.
    pub fn resolve(
        &self,
        parent_text: Option<&TextStyle>,
        classes: &[ClassName],
        inline: &StyleRefinement,
        state: InteractionState,
        local: Option<&Stylesheet>,
    ) -> Style {
        let inherited = parent_text
            .cloned()
            .unwrap_or_else(|| TextStyle::from_tokens(&self.tokens));
        let mut style = Style::inheriting(inherited);
        let rules: Vec<&ClassRule> = classes
            .iter()
            .filter_map(|class| {
                let name = class.as_str();
                local
                    .and_then(|sheet| sheet.rule(name))
                    .or_else(|| self.classes.rule(name))
            })
            .collect();
        let apply = |style: &mut Style, layer: &Option<StyleRefinement>| {
            if let Some(refinement) = layer {
                refinement.apply_to(style, &self.tokens);
            }
        };
        for rule in &rules {
            apply(&mut style, &rule.base);
        }
        if state.disabled {
            for rule in &rules {
                apply(&mut style, &rule.disabled);
            }
        } else {
            if state.hovered {
                for rule in &rules {
                    apply(&mut style, &rule.hover);
                }
            }
            if state.focused {
                for rule in &rules {
                    apply(&mut style, &rule.focus);
                }
            }
            if state.pressed {
                for rule in &rules {
                    apply(&mut style, &rule.active);
                }
            }
        }
        inline.apply_to(&mut style, &self.tokens);
        style
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
            None,
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
                    InteractionState {
                        hovered,
                        pressed,
                        focused: false,
                        disabled: false,
                    },
                    None,
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
            None,
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
            None,
        );
        let child = theme.resolve(
            Some(&parent.text),
            &[],
            &StyleRefinement::default(),
            InteractionState::default(),
            None,
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
                None,
            )
        };
        let light = resolve(&Theme::light());
        let dark = resolve(&Theme::dark());
        let change = dark.diff(&light);
        assert!(change.paint && !change.layout);
    }

    #[test]
    fn local_sheet_replaces_the_same_theme_class() {
        let theme = Theme::light();
        let local = Stylesheet::default()
            .with_class("btn", StyleRefinement::default().background(Color::BLACK));
        let style = theme.resolve(
            None,
            &classes("btn"),
            &StyleRefinement::default(),
            InteractionState {
                hovered: true,
                pressed: false,
                focused: false,
                disabled: false,
            },
            Some(&local),
        );
        assert_eq!(style.visual.background, Color::BLACK);
        assert!(!local.has_class("missing"));
    }

    #[test]
    fn unknown_classes_are_ignored() {
        let theme = Theme::light();
        let style = theme.resolve(
            None,
            &classes("does-not-exist"),
            &StyleRefinement::default(),
            InteractionState::default(),
            None,
        );
        assert_eq!(style.visual.background, Color::TRANSPARENT);
        assert!(!theme.has_class("does-not-exist"));
    }
}
