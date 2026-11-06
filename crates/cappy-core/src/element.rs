//! Declarative description of a UI tree. An [`Element`] is mounted into a [`crate::Ui`] once;
//! afterwards only its reactive bindings re-run.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use cappy_style::{ClassName, StyleRefinement, Stylesheet};

use crate::ClickEvent;
use crate::binding::{Dynamic, IntoClasses, IntoStyle, IntoText};

pub(crate) type ClickHandler = Box<dyn FnMut(&ClickEvent)>;
pub(crate) type HoverHandler = Box<dyn FnMut(bool)>;
pub(crate) type TextBinding = Box<dyn Fn() -> String>;
pub(crate) type ClassBinding = Box<dyn Fn() -> Vec<ClassName>>;
pub(crate) type StyleBinding = Box<dyn Fn() -> StyleRefinement>;

pub(crate) enum ElementContent {
    Container,
    StaticText(Arc<str>),
    DynamicText(TextBinding),
    /// Transparent hole. Mount skips it until a parent replaces it with a child.
    Slot {
        index: u16,
    },
}

/// Identity of one `#[slot]` field. The component stores the marker, not the child, so it stays
/// [`Copy`](Copy).
pub struct Slot<C, T> {
    pub(crate) index: u16,
    _marker: PhantomData<fn() -> (C, T)>,
}

impl<C, T> Copy for Slot<C, T> {}

impl<C, T> Clone for Slot<C, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C, T> fmt::Debug for Slot<C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slot").field("index", &self.index).finish()
    }
}

impl<C, T> Slot<C, T> {
    #[must_use]
    pub const fn new(index: u16) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }
}

/// Set on the root a component renders. A slot walk does not enter a nested component root.
const COMPONENT_SLOT_OWNER: u32 = 1;

/// A node description: a container (`div`), a text leaf, or a slot hole, with classes, style and
/// handlers.
pub struct Element {
    pub(crate) content: ElementContent,
    pub(crate) key: Option<String>,
    pub(crate) classes: Vec<ClassName>,
    pub(crate) class_binding: Option<ClassBinding>,
    pub(crate) style: StyleRefinement,
    pub(crate) style_binding: Option<StyleBinding>,
    /// Sheet for this element only. Children do not inherit it.
    pub(crate) stylesheet: Option<&'static Stylesheet>,
    pub(crate) on_click: Option<ClickHandler>,
    pub(crate) on_hover: Option<HoverHandler>,
    pub(crate) children: Vec<Element>,
    /// Non-zero on a component root. Nested slot walks stop at a different owner.
    pub(crate) slot_owner: u32,
    /// The rendered tree contains slot holes, so unnamed children fill the first slot.
    pub(crate) accepts_slots: bool,
}

/// A container element.
pub fn div() -> Element {
    Element::new(ElementContent::Container)
}

/// A text leaf. A string never changes; a [`Dynamic`] closure is recomputed whenever a signal it
/// reads changes.
pub fn text(content: impl IntoText) -> Element {
    content.into_text()
}

/// A text leaf whose content is recomputed whenever a signal it reads changes.
pub fn text_dyn(content: impl Fn() -> String + 'static) -> Element {
    text(Dynamic(content))
}

/// A hole where a `#[slot]` field is placed. `slot(self.body)` reads that field. The parent
/// replaces the hole; an unfilled hole is omitted at mount.
#[must_use]
pub fn slot<C, T>(slot: Slot<C, T>) -> Element {
    Element::new(ElementContent::Slot { index: slot.index })
}

impl Element {
    pub(crate) fn new(content: ElementContent) -> Self {
        Self {
            content,
            key: None,
            classes: Vec::new(),
            class_binding: None,
            style: StyleRefinement::default(),
            style_binding: None,
            stylesheet: None,
            on_click: None,
            on_hover: None,
            children: Vec::new(),
            slot_owner: 0,
            accepts_slots: false,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(
            self.content,
            ElementContent::StaticText(_) | ElementContent::DynamicText(_)
        )
    }

    /// Adds classes from a space-separated list, e.g. `"btn btn-primary"`, or a single typed
    /// class from `#[derive(ClassName)]`. A [`Dynamic`] closure is recomputed whenever a signal it
    /// reads changes; dynamic classes apply after static ones.
    #[must_use]
    pub fn class(mut self, classes: impl IntoClasses) -> Self {
        classes.add_to(&mut self);
        self
    }

    /// Classes recomputed whenever a signal they read changes. Applied after static classes.
    #[must_use]
    pub fn class_dyn(self, classes: impl Fn() -> String + 'static) -> Self {
        self.class(Dynamic(classes))
    }

    /// Inline style, layered over the classes. A [`Dynamic`] closure is recomputed whenever a
    /// signal it reads changes and layers over the fixed inline style.
    #[must_use]
    pub fn style(mut self, style: impl IntoStyle) -> Self {
        style.add_to(&mut self);
        self
    }

    /// Inline style recomputed whenever a signal it reads changes. Layered over `style`.
    #[must_use]
    pub fn style_dyn(self, refinement: impl Fn() -> StyleRefinement + 'static) -> Self {
        self.style(Dynamic(refinement))
    }

    /// Stylesheet for this element only. Class names it defines win over the theme on this
    /// element; child elements keep resolving against the theme.
    #[must_use]
    pub fn stylesheet(mut self, stylesheet: &'static Stylesheet) -> Self {
        self.stylesheet = Some(stylesheet);
        self
    }

    #[must_use]
    pub fn on_click(mut self, handler: impl FnMut(&ClickEvent) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Called with `true` when the pointer enters this element and `false` when it leaves.
    #[must_use]
    pub fn on_hover(mut self, handler: impl FnMut(bool) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }

    /// Identifier for lookups such as [`crate::Ui::find_by_key`].
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Appends a child.
    ///
    /// # Panics
    /// Text elements are leaves; adding a child to one panics.
    #[must_use]
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        assert!(!self.is_text(), "text elements cannot have children");
        self.children.push(child.into());
        self
    }

    #[must_use]
    pub fn children<I>(self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Element>,
    {
        children
            .into_iter()
            .fold(self, |element, child| element.child(child))
    }

    /// Remembers that this component rendered slot holes. Later unnamed children then fill the
    /// first slot instead of appending to the root.
    #[doc(hidden)]
    #[must_use]
    pub fn prepare_slots(mut self) -> Self {
        if self.slot_owner != 0 && self.has_owned_slot() {
            self.accepts_slots = true;
        }
        self
    }

    /// Replaces the hole for `slot` with `child`. A later fill of the same slot is dropped.
    /// The walk does not enter a nested component.
    #[must_use]
    pub fn fill_slot<C, T>(self, slot: Slot<C, T>, child: impl Into<Element>) -> Self {
        self.fill_index(slot.index, child.into())
    }

    /// Appends `child`, or fills the first slot when [`prepare_slots`](Self::prepare_slots) found
    /// holes. Further unnamed children are dropped once a component accepts slots.
    #[doc(hidden)]
    #[must_use]
    pub fn place_child(self, child: impl Into<Element>) -> Self {
        let child = child.into();
        if self.accepts_slots {
            self.fill_index(0, child)
        } else {
            self.child(child)
        }
    }

    /// [`place_child`](Self::place_child) for every element `children` yields.
    #[doc(hidden)]
    #[must_use]
    pub fn place_children<I>(self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Element>,
    {
        children
            .into_iter()
            .fold(self, |element, child| element.place_child(child))
    }

    fn fill_index(mut self, index: u16, child: Element) -> Self {
        let accepts_slots = self.accepts_slots;
        let slot_owner = self.slot_owner;
        let mut incoming = Some(child);
        let replaced_self = self.replace_slot(index, &mut incoming, true);
        if replaced_self {
            self.accepts_slots = accepts_slots;
            if self.slot_owner == 0 {
                self.slot_owner = slot_owner;
            }
        }
        self
    }

    /// Returns whether this element itself was the matching hole and was replaced.
    fn replace_slot(&mut self, index: u16, incoming: &mut Option<Element>, is_root: bool) -> bool {
        if incoming.is_none() || (!is_root && self.slot_owner != 0) {
            return false;
        }
        if let ElementContent::Slot { index: slot_index } = self.content
            && slot_index == index
            && let Some(child) = incoming.take()
        {
            *self = child;
            return true;
        }
        for child in &mut self.children {
            if incoming.is_none() {
                break;
            }
            child.replace_slot(index, incoming, false);
        }
        false
    }

    fn has_owned_slot(&self) -> bool {
        self.finds_owned_slot(true)
    }

    fn finds_owned_slot(&self, is_root: bool) -> bool {
        if !is_root && self.slot_owner != 0 {
            return false;
        }
        if matches!(self.content, ElementContent::Slot { .. }) {
            return true;
        }
        self.children
            .iter()
            .any(|child| child.finds_owned_slot(false))
    }
}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.content {
            ElementContent::Container => "div".to_owned(),
            ElementContent::StaticText(text) => format!("text({text:?})"),
            ElementContent::DynamicText(_) => "text(<dynamic>)".to_owned(),
            ElementContent::Slot { index } => format!("slot({index})"),
        };
        f.debug_struct("Element")
            .field("kind", &kind)
            .field("key", &self.key)
            .field("classes", &self.classes)
            .field("children", &self.children)
            .finish_non_exhaustive()
    }
}

/// A reusable piece of UI that renders to an [`Element`].
///
/// [`stylesheet`](Component::stylesheet) is stamped onto every node `render` returns that does not
/// already carry a sheet. It is not registered on the theme. A class the sheet does not define
/// still resolves against the theme. [`Stylesheet::empty`] is not stamped.
pub trait Component {
    fn render(self) -> Element;

    fn stylesheet() -> &'static Stylesheet {
        Stylesheet::empty()
    }
}

impl<C: Component> From<C> for Element {
    fn from(component: C) -> Self {
        let mut element = component.render();
        element.slot_owner = COMPONENT_SLOT_OWNER;
        let sheet = C::stylesheet();
        if !std::ptr::eq(sheet, Stylesheet::empty()) {
            stamp_stylesheet(&mut element, sheet);
        }
        element
    }
}

fn stamp_stylesheet(element: &mut Element, sheet: &'static Stylesheet) {
    if element.stylesheet.is_none() {
        element.stylesheet = Some(sheet);
    }
    for child in &mut element.children {
        stamp_stylesheet(child, sheet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Badge(&'static str);

    impl Component for Badge {
        fn render(self) -> Element {
            text(self.0).class("badge")
        }
    }

    #[test]
    fn class_lists_accumulate() {
        let element = div().class("btn").class("btn-primary large");
        let names: Vec<&str> = element.classes.iter().map(ClassName::as_str).collect();
        assert_eq!(names, ["btn", "btn-primary", "large"]);
    }

    #[test]
    fn components_convert_into_elements() {
        let element = div().child(Badge("new"));
        assert!(element.children[0].is_text());
        assert_eq!(element.children[0].classes[0].as_str(), "badge");
    }

    #[test]
    #[should_panic(expected = "text elements cannot have children")]
    fn text_cannot_have_children() {
        let _ = text("leaf").child(div());
    }

    fn owned(element: Element) -> Element {
        let mut element = element;
        element.slot_owner = COMPONENT_SLOT_OWNER;
        element
    }

    fn hole(index: u16) -> Element {
        slot(Slot::<(), Element>::new(index))
    }

    #[test]
    fn unnamed_children_append_when_the_tree_has_no_slots() {
        let element = owned(div())
            .prepare_slots()
            .place_child(text("a"))
            .place_child(text("b"));
        assert_eq!(element.children.len(), 2);
    }

    #[test]
    fn the_first_unnamed_child_fills_the_first_slot() {
        let element = owned(div().child(hole(0)).child(text("keep").key("keep")))
            .prepare_slots()
            .place_child(text("body").key("body"))
            .place_child(text("ignored").key("ignored"));
        assert_eq!(element.children.len(), 2);
        assert_eq!(element.children[0].key.as_deref(), Some("body"));
        assert_eq!(element.children[1].key.as_deref(), Some("keep"));
    }

    #[test]
    fn a_named_fill_does_not_enter_a_nested_component() {
        let nested = owned(div().child(hole(0)));
        let outer = owned(div().child(hole(0)).child(nested))
            .prepare_slots()
            .fill_slot(Slot::<(), Element>::new(0), text("outer").key("outer"));
        assert_eq!(outer.children[0].key.as_deref(), Some("outer"));
        assert!(matches!(
            outer.children[1].children[0].content,
            ElementContent::Slot { index: 0 }
        ));
    }

    #[test]
    fn replacing_a_slot_root_drops_later_unnamed_children() {
        let element = owned(hole(0))
            .prepare_slots()
            .place_child(text("only").key("only"))
            .place_child(text("extra").key("extra"));
        assert_eq!(element.key.as_deref(), Some("only"));
        assert!(element.children.is_empty());
    }
}
