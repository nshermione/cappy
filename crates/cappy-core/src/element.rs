//! Declarative description of a UI tree. An [`Element`] is mounted into a [`crate::Ui`] once.
//! Text, class and style bindings keep running after that. [`show`](crate::show), [`if_else`]
//! and [`for_each`] can still change structure: `show` only hides, and `if_else` / `for_each`
//! dispose the parts they remove.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use cappy_style::{ClassName, StyleRefinement, Stylesheet};

use crate::ClickEvent;
use crate::binding::{Dynamic, IntoClasses, IntoStyle, IntoText};
use crate::bitmap::{ImagePixels, ObjectFit};

pub(crate) type ClickHandler = Box<dyn FnMut(&ClickEvent)>;
pub(crate) type HoverHandler = Box<dyn FnMut(bool)>;
pub(crate) type TextBinding = Box<dyn Fn() -> String>;
pub(crate) type ClassBinding = Box<dyn Fn() -> Vec<ClassName>>;
pub(crate) type StyleBinding = Box<dyn Fn() -> StyleRefinement>;

pub(crate) enum ElementContent {
    Container,
    StaticText(Arc<str>),
    DynamicText(TextBinding),
    Image {
        pixels: Arc<ImagePixels>,
        fit: ObjectFit,
    },
    /// Transparent hole. Mount skips it until a parent replaces it with a child.
    Slot {
        index: u16,
    },
    /// Several elements spliced into the parent, with no node of their own.
    Fragment(Vec<Element>),
    /// `show`, `if` / `else` or `for`. The runtime splices the result into the parent.
    Flow(crate::ui::flow::FlowSpec),
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
    pub(crate) stylesheet: Option<Arc<Stylesheet>>,
    pub(crate) on_click: Option<ClickHandler>,
    pub(crate) on_hover: Option<HoverHandler>,
    pub(crate) children: Vec<Element>,
    /// Non-zero on a component root. Nested slot walks stop at a different owner.
    pub(crate) slot_owner: u32,
    /// The rendered tree contains slot holes, so unnamed children fill the first slot.
    pub(crate) accepts_slots: bool,
    /// Hooks from the component that rendered this element. Mount moves them onto the runtime.
    pub(crate) lifecycle: Option<crate::lifecycle::Lifecycle>,
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

/// An image leaf. `pixels` are straight RGBA8. Give the element a width and height; the box does
/// not yet size itself from the bitmap. [`Element::fit`] chooses how the bitmap fills that box.
#[must_use]
pub fn image(pixels: impl Into<Arc<ImagePixels>>) -> Element {
    Element::new(ElementContent::Image {
        pixels: pixels.into(),
        fit: ObjectFit::Fill,
    })
}

/// A hole where a `#[slot]` field is placed. `slot(self.body)` reads that field. The parent
/// replaces the hole; an unfilled hole is omitted at mount.
#[must_use]
pub fn slot<C, T>(slot: Slot<C, T>) -> Element {
    Element::new(ElementContent::Slot { index: slot.index })
}

/// Splices `children` into the parent. The fragment itself is not a layout node.
#[must_use]
pub fn fragment(children: impl IntoIterator<Item = impl Into<Element>>) -> Element {
    Element::new(ElementContent::Fragment(
        children.into_iter().map(Into::into).collect(),
    ))
}

/// Mounts `view` once. When `when` is false the nodes stay, with their signals and bindings, but
/// layout treats them as [`Display::None`](cappy_style::Display::None).
///
/// `when` is the only value this node tracks. `view` runs outside that tracking, so signals it
/// reads subscribe on their own bindings. Hiding does not dispose anything. A parent `if_else` or
/// `for_each` that drops this `show` disposes it, hidden or not. Keep [`slot`] out of `view`:
/// slots are filled before mount.
#[must_use]
pub fn show(when: impl Fn() -> bool + 'static, view: impl Fn() -> Element + 'static) -> Element {
    Element::new(ElementContent::Flow(crate::ui::flow::show_spec(when, view)))
}

/// Gives `view` the caller's [`Context`](crate::Context) without the `'static` closure borrowing it.
///
/// `element!` uses this for `show` and `if`. The closure receives `&Context` when it runs, and that
/// call also enters the context so [`Context::current`](crate::Context::current) resolves to it.
#[doc(hidden)]
pub fn with_context(
    context: &crate::Context,
    view: impl Fn(&crate::Context) -> Element + 'static,
) -> impl Fn() -> Element + 'static {
    let context = context.clone();
    move || context.enter(|| view(&context))
}

/// Keeps the branch `when` selects and disposes the other one, including every nested `show`,
/// `for_each` and `if_else`. Coming back mounts that branch again.
///
/// `when` is the only value this node tracks. Both builders run outside that tracking.
/// Keep [`slot`] out of either branch: slots are filled before mount.
#[must_use]
pub fn if_else(
    when: impl Fn() -> bool + 'static,
    then_branch: impl Fn() -> Element + 'static,
    else_branch: impl Fn() -> Element + 'static,
) -> Element {
    Element::new(ElementContent::Flow(crate::ui::flow::branch_spec(
        when,
        then_branch,
        else_branch,
    )))
}

/// Repeats `view` for each item `read` visits. Rows with the same key are kept: their node and
/// the signal passed to `view` stay, and the signal updates only when the item is not equal.
/// A missing key is disposed, including anything nested in that row.
///
/// `read` must visit items while it still holds them, usually from [`Signal::with`](crate::Signal::with).
/// `key_of` borrows the key from the item. A duplicate key panics. Keep [`slot`] out of `view`:
/// slots are filled before mount.
///
/// # Panics
/// Panics when two items in one pass share a key.
#[must_use]
pub fn for_each<T, K>(
    read: impl Fn(&mut dyn FnMut(&T)) + 'static,
    key_of: impl for<'a> Fn(&'a T) -> &'a K + 'static,
    view: impl Fn(crate::Signal<T>) -> Element + 'static,
) -> Element
where
    T: Clone + PartialEq + 'static,
    K: ToOwned + Eq + std::hash::Hash + ?Sized + 'static,
    K::Owned: Clone + Eq + std::hash::Hash + std::borrow::Borrow<K> + 'static,
{
    Element::new(ElementContent::Flow(crate::ui::flow::repeat_spec(
        read, key_of, view,
    )))
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
            lifecycle: None,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(
            self.content,
            ElementContent::StaticText(_) | ElementContent::DynamicText(_)
        )
    }

    /// How the bitmap fills the box. Only image elements use this.
    ///
    /// # Panics
    /// Panics when the element is not an image.
    #[must_use]
    pub fn fit(mut self, fit: ObjectFit) -> Self {
        let ElementContent::Image { fit: slot, .. } = &mut self.content else {
            panic!("fit() is only for image elements");
        };
        *slot = fit;
        self
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
    pub fn stylesheet(mut self, stylesheet: Arc<Stylesheet>) -> Self {
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
    pub fn child(mut self, child: impl IntoElement) -> Self {
        assert!(!self.is_text(), "text elements cannot have children");
        assert!(
            !matches!(self.content, ElementContent::Image { .. }),
            "image elements cannot have children"
        );
        self.children.push(child.into_element());
        self
    }

    #[must_use]
    pub fn children<I>(self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: IntoElement,
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
    pub fn fill_slot<C, T>(self, slot: Slot<C, T>, child: impl IntoElement) -> Self {
        self.fill_index(slot.index, child.into_element())
    }

    /// Appends `child`, or fills the first slot when [`prepare_slots`](Self::prepare_slots) found
    /// holes. Further unnamed children are dropped once a component accepts slots.
    #[doc(hidden)]
    #[must_use]
    pub fn place_child(self, child: impl IntoElement) -> Self {
        let child = child.into_element();
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
        I::Item: IntoElement,
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
            ElementContent::Image { .. } => "image".to_owned(),
            ElementContent::Slot { index } => format!("slot({index})"),
            ElementContent::Fragment(children) => format!("fragment({})", children.len()),
            ElementContent::Flow(_) => "flow".to_owned(),
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
/// still resolves against the theme. `None` is not stamped.
pub trait HasContext {
    fn context(&self) -> &crate::Context;
}

pub trait Component: HasContext + Clone {
    fn render(&self) -> Element;

    fn stylesheet() -> Option<Arc<Stylesheet>> {
        None
    }
}

/// Converts a builder or component into the [`Element`] stored in the tree.
///
/// [`Element`] returns itself. A [`Component`] renders, then marks that root so a slot walk stops
/// there. [`Container`], [`Text`] and [`Image`] do not: they call [`div`], [`text`] and [`image`].
pub trait IntoElement {
    fn into_element(self) -> Element;
}

impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
    }
}

impl<C: Component + 'static> IntoElement for C {
    fn into_element(self) -> Element {
        let context = self.context().clone();
        context.enter(|| {
            context.with(&self, || {
                let guard = context.runtime.begin_hooks();
                let mut element = self.render();
                if let Some(mut hooks) = guard.finish() {
                    hooks.inner = element.lifecycle.take().map(Box::new);
                    element.lifecycle = Some(hooks);
                }
                element.slot_owner = COMPONENT_SLOT_OWNER;
                if let Some(sheet) = C::stylesheet() {
                    stamp_stylesheet(&mut element, &sheet);
                }
                element
            })
        })
    }
}

/// `element!` `div` builder. [`IntoElement`] calls [`div`] and leaves `slot_owner` at 0, so a
/// parent component can still see slot holes nested inside.
pub struct Container {
    on_click: Option<ClickHandler>,
    on_hover: Option<HoverHandler>,
    stylesheet: Option<Arc<Stylesheet>>,
}

impl Container {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self {
            on_click: None,
            on_hover: None,
            stylesheet: None,
        }
    }

    #[must_use]
    pub fn on_click(mut self, handler: impl FnMut(&ClickEvent) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    #[must_use]
    pub fn on_hover(mut self, handler: impl FnMut(bool) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }

    #[must_use]
    pub fn stylesheet(mut self, stylesheet: Arc<Stylesheet>) -> Self {
        self.stylesheet = Some(stylesheet);
        self
    }
}

impl IntoElement for Container {
    fn into_element(self) -> Element {
        let mut element = div();
        element.on_click = self.on_click;
        element.on_hover = self.on_hover;
        element.stylesheet = self.stylesheet;
        element
    }
}

/// `element!` `text` builder. [`open`](Text::open) has no content yet, so only [`Text::content`]
/// implements [`IntoElement`].
pub struct Text<T> {
    content: T,
}

impl Text<()> {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self { content: () }
    }
}

impl<T> Text<T> {
    #[must_use]
    pub fn content<U>(self, content: U) -> Text<U> {
        Text { content }
    }
}

impl<T: IntoText> IntoElement for Text<T> {
    fn into_element(self) -> Element {
        text(self.content)
    }
}

/// `element!` `image` builder. [`open`](Image::open) has no pixels yet, so only [`Image::content`]
/// implements [`IntoElement`].
pub struct Image<T> {
    content: T,
    fit: ObjectFit,
}

impl Image<()> {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self {
            content: (),
            fit: ObjectFit::Fill,
        }
    }
}

impl<T> Image<T> {
    #[must_use]
    pub fn content<U>(self, content: U) -> Image<U> {
        Image {
            content,
            fit: self.fit,
        }
    }

    #[must_use]
    pub fn fit(mut self, fit: ObjectFit) -> Self {
        self.fit = fit;
        self
    }
}

impl<T: Into<Arc<ImagePixels>>> IntoElement for Image<T> {
    fn into_element(self) -> Element {
        image(self.content).fit(self.fit)
    }
}

fn stamp_stylesheet(element: &mut Element, sheet: &Arc<Stylesheet>) {
    if element.stylesheet.is_none() {
        element.stylesheet = Some(Arc::clone(sheet));
    }
    for child in &mut element.children {
        stamp_stylesheet(child, sheet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Badge {
        context: crate::Context,
        text: &'static str,
    }

    impl HasContext for Badge {
        fn context(&self) -> &crate::Context {
            &self.context
        }
    }

    impl Component for Badge {
        fn render(&self) -> Element {
            text(self.text).class("badge")
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
        let element = div().child(Badge {
            context: crate::Application::new().context().clone(),
            text: "new",
        });
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
    fn container_into_element_is_not_a_slot_boundary() {
        let element = Container::open(&crate::Application::new().context()).into_element();
        assert_eq!(element.slot_owner, 0);
    }

    #[test]
    fn a_slot_nested_in_divs_is_still_filled() {
        let element = owned(div().child(div().child(hole(0))))
            .prepare_slots()
            .place_child(text("body").key("body"));
        assert_eq!(element.children[0].children[0].key.as_deref(), Some("body"));
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
