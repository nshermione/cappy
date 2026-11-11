//! Declarative description of a UI tree. An [`Element`] is mounted into a [`crate::Ui`] once.
//! Text, class and style bindings keep running after that. [`show`](crate::show), [`if_else`]
//! and [`for_each`] can still change structure: `show` only hides, and `if_else` / `for_each`
//! dispose the parts they remove.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use cappy_layout::Point;
use cappy_style::{ClassName, StyleRefinement, Stylesheet};

use crate::binding::{Dynamic, IntoClasses, IntoStyle, IntoText, LiveText};
use crate::bitmap::ObjectFit;
use crate::clipboard::Clipboard;
use crate::image_source::ImageSource;
use crate::{ClickEvent, ImeEvent, KeyEvent, PointerEvent};

pub(crate) type ClickHandler = Box<dyn FnMut(&ClickEvent)>;
pub(crate) type HoverHandler = Box<dyn FnMut(bool)>;
pub(crate) type KeyHandler = Box<dyn FnMut(&KeyEvent, &mut dyn Clipboard)>;
pub(crate) type TextHandler = Box<dyn FnMut(&str)>;
pub(crate) type ImeHandler = Box<dyn FnMut(&ImeEvent)>;
pub(crate) type FocusHandler = Box<dyn FnMut(bool)>;
pub(crate) type PointerHandler = Box<dyn FnMut(&PointerEvent)>;
pub(crate) type InputHandler = Box<dyn FnMut(&str)>;
pub(crate) type TextBinding = Box<dyn Fn() -> String>;
pub(crate) type CaretBinding = Box<dyn Fn() -> Option<usize>>;
pub(crate) type SelectionBinding = Box<dyn Fn() -> Option<(usize, usize)>>;
pub(crate) type ClassBinding = Box<dyn Fn() -> Vec<ClassName>>;
pub(crate) type StyleBinding = Box<dyn Fn() -> StyleRefinement>;
pub(crate) type DisabledBinding = Box<dyn Fn() -> bool>;
pub(crate) type PlaceBinding = Box<dyn Fn() -> Point>;

pub(crate) enum ElementContent {
    Container,
    StaticText(Arc<str>),
    DynamicText(TextBinding),
    Image {
        source: ImageSource,
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
    Input(InputSpec),
}

/// Value of an [`input`]: a fixed string, or a signal that is read and written.
#[doc(hidden)]
pub enum InputValue {
    Fixed(String),
    Bound {
        read: TextBinding,
        write: Box<dyn Fn(String)>,
    },
}

/// Fields of an [`input`] before it is mounted.
#[doc(hidden)]
pub struct InputSpec {
    pub value: InputValue,
    pub placeholder: String,
    pub on_input: Option<InputHandler>,
    pub on_submit: Option<InputHandler>,
    pub disabled: bool,
    pub disabled_when: Option<DisabledBinding>,
}

impl InputSpec {
    fn empty() -> Self {
        Self {
            value: InputValue::Fixed(String::new()),
            placeholder: String::new(),
            on_input: None,
            on_submit: None,
            disabled: false,
            disabled_when: None,
        }
    }
}

/// A static string, or a [`Dynamic`] value. The closure may return a string or a [`Signal`] /
/// [`Reactive`] string. A signal is read on each update and written on each edit.
pub trait IntoInputValue {
    fn into_input_value(self) -> InputValue;
}

impl IntoInputValue for String {
    fn into_input_value(self) -> InputValue {
        InputValue::Fixed(self)
    }
}

impl IntoInputValue for &str {
    fn into_input_value(self) -> InputValue {
        InputValue::Fixed(self.to_owned())
    }
}

impl<F, R> IntoInputValue for Dynamic<F>
where
    F: Fn() -> R + 'static,
    R: LiveText + 'static,
{
    fn into_input_value(self) -> InputValue {
        let Dynamic(source) = self;
        let source = Rc::new(source);
        let read = Rc::clone(&source);
        InputValue::Bound {
            read: Box::new(move || (*read)().read_text()),
            write: Box::new(move |value| (*source)().write_text(value)),
        }
    }
}

/// A fixed top-left, or a [`Dynamic`] one, for [`Element::place`].
pub trait IntoPlace {
    fn apply_place(self, element: &mut Element);
}

impl IntoPlace for Point {
    fn apply_place(self, element: &mut Element) {
        element.place = Some(self);
        element.place_binding = None;
    }
}

impl<F> IntoPlace for Dynamic<F>
where
    F: Fn() -> Point + 'static,
{
    fn apply_place(self, element: &mut Element) {
        let Dynamic(read) = self;
        element.place = None;
        element.place_binding = Some(Box::new(read));
    }
}

/// A fixed disabled flag, or a [`Dynamic`] one, for [`Element::disabled`].
pub trait IntoDisabled {
    fn apply_disabled(self, element: &mut Element);
}

impl IntoDisabled for bool {
    fn apply_disabled(self, element: &mut Element) {
        element.disabled = self;
        element.disabled_when = None;
        if let ElementContent::Input(spec) = &mut element.content {
            spec.disabled = self;
            spec.disabled_when = None;
        }
    }
}

impl<F> IntoDisabled for Dynamic<F>
where
    F: Fn() -> bool + 'static,
{
    fn apply_disabled(self, element: &mut Element) {
        let Dynamic(read) = self;
        element.disabled_when = Some(Box::new(read));
    }
}

/// A fixed caret index, or a [`Dynamic`] one, for [`Element::caret`].
pub trait IntoCaret {
    fn add_caret(self, element: &mut Element);
}

impl IntoCaret for usize {
    fn add_caret(self, element: &mut Element) {
        element.caret = Some(self);
        element.caret_binding = None;
    }
}

impl<F> IntoCaret for Dynamic<F>
where
    F: Fn() -> usize + 'static,
{
    fn add_caret(self, element: &mut Element) {
        let Dynamic(read) = self;
        element.caret_binding = Some(Box::new(move || Some(read())));
    }
}

/// A fixed selection, or a [`Dynamic`] one, for [`Element::selection`].
pub trait IntoSelection {
    fn add_selection(self, element: &mut Element);
}

impl IntoSelection for (usize, usize) {
    fn add_selection(self, element: &mut Element) {
        element.selection = Some(self);
        element.selection_binding = None;
    }
}

impl<F> IntoSelection for Dynamic<F>
where
    F: Fn() -> (usize, usize) + 'static,
{
    fn add_selection(self, element: &mut Element) {
        let Dynamic(read) = self;
        element.selection_binding = Some(Box::new(move || Some(read())));
    }
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

/// Where keyboard focus may move inside an element.
///
/// Without a scope, Tab visits every focusable node. [`Trap`](Self::Trap) keeps Tab inside a
/// modal. [`Vertical`](Self::Vertical) and [`Horizontal`](Self::Horizontal) are the roving
/// tabindex used by Select, Menu, Tabs, and Tree: arrow keys move inside the scope, Tab stops
/// on the active item once, and closing the scope returns focus to the node that had it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusScope {
    /// Tab and Shift+Tab cycle through this element and do not leave it.
    ///
    /// The last visible trap in tree order is the active one, so a trap nested inside another
    /// keeps Tab in the inner trap. A trap hidden with `display: none` or `show(false)` is not
    /// active. When focus is outside the active trap, the next Tab enters it.
    Trap,
    /// Up and Down move among the focusable descendants. Left and Right reach the focused node.
    ///
    /// Only the active descendant is in the tab order: the focused one, else the last one that
    /// had focus, else the first. When this scope becomes visible and some other node is focused,
    /// focus moves to that descendant and that other node is remembered. Hiding or disposing the
    /// scope gives focus back, unless focus has already left. A scope mounted while nothing is
    /// focused does not take it.
    Vertical,
    /// Left and Right move among the focusable descendants. Up and Down reach the focused node.
    ///
    /// A focused input keeps Left and Right for its caret. Tab order and returning focus match
    /// [`Vertical`](Self::Vertical).
    Horizontal,
}

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
    pub(crate) focusable: bool,
    /// Keyboard scope. `None` leaves this element in the flat tab order.
    pub(crate) focus_scope: Option<FocusScope>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) on_text: Option<TextHandler>,
    pub(crate) on_ime: Option<ImeHandler>,
    pub(crate) on_focus: Option<FocusHandler>,
    pub(crate) on_pointer: Option<PointerHandler>,
    /// Primary press that landed outside this element. Runs after that press's click.
    pub(crate) on_dismiss: Option<ClickHandler>,
    /// Fixed disabled flag. A [`disabled_when`](Element::disabled_when) binding overwrites this
    /// after mount.
    pub(crate) disabled: bool,
    pub(crate) disabled_when: Option<DisabledBinding>,
    pub(crate) caret: Option<usize>,
    pub(crate) caret_binding: Option<CaretBinding>,
    pub(crate) selection: Option<(usize, usize)>,
    pub(crate) selection_binding: Option<SelectionBinding>,
    pub(crate) children: Vec<Element>,
    /// Non-zero on a component root. Nested slot walks stop at a different owner.
    pub(crate) slot_owner: u32,
    /// The rendered tree contains slot holes, so unnamed children fill the first slot.
    pub(crate) accepts_slots: bool,
    /// Hooks from the component that rendered this element. Mount moves them onto the runtime.
    pub(crate) lifecycle: Option<crate::lifecycle::Lifecycle>,
    /// Painted and hit above the tree, outside ancestor overflow, and out of parent flow.
    pub(crate) overlay: bool,
    /// Fixed top-left for [`Element::overlay`]. A binding overwrites this after mount.
    pub(crate) place: Option<Point>,
    pub(crate) place_binding: Option<PlaceBinding>,
}

/// A container element.
pub fn div() -> Element {
    Element::new(ElementContent::Container)
}

/// A text leaf. A string never changes. A [`Dynamic`] closure (`:value` in `element!`) is
/// recomputed when a signal it reads changes. A [`Signal`] or [`Reactive`] string is read
/// without calling [`Signal::get`](crate::Signal::get).
pub fn text(content: impl IntoText) -> Element {
    content.into_text()
}

/// A text leaf whose content is recomputed whenever a signal it reads changes.
pub fn text_dyn(content: impl Fn() -> String + 'static) -> Element {
    text(Dynamic(content))
}

/// A single-line text field. Click to place the caret and type. `value = "hello"` stays as
/// written. `:value = name` follows a [`Signal`](crate::Signal) or [`Reactive`](crate::Reactive)
/// string and writes edits back.
#[must_use]
pub fn input() -> Element {
    let mut element = Element::new(ElementContent::Input(InputSpec::empty()));
    element.focusable = true;
    element
}

/// An image leaf. `source` is pixels ([`ImageClip`], [`ImagePixels`](crate::ImagePixels)) or a
/// locator string. [`asset_path!`](crate::asset_path) turns a path relative to the calling crate
/// into that locator. A path is read from disk. `http://` and `https://` are fetched. In the
/// browser any other string is a URL on the page, such as `assets/photo.png`. A GIF with more
/// than one frame plays. Give the element a width and height; the box does not yet size itself from the
/// bitmap. [`Element::fit`] chooses how the bitmap fills that box.
#[must_use]
pub fn image(source: impl Into<ImageSource>) -> Element {
    Element::new(ElementContent::Image {
        source: source.into(),
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
            focusable: false,
            focus_scope: None,
            on_key: None,
            on_text: None,
            on_ime: None,
            on_focus: None,
            on_pointer: None,
            on_dismiss: None,
            disabled: false,
            disabled_when: None,
            caret: None,
            caret_binding: None,
            selection: None,
            selection_binding: None,
            children: Vec::new(),
            slot_owner: 0,
            accepts_slots: false,
            lifecycle: None,
            overlay: false,
            place: None,
            place_binding: None,
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

    /// Lets this element take keyboard focus. An [`input`] is focusable without this call.
    #[must_use]
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// Keeps Tab inside this element, or moves arrow keys among its descendants.
    ///
    /// See [`FocusScope`]. A modal uses [`FocusScope::Trap`]. Select, Menu, Tabs, and Tree use
    /// [`FocusScope::Vertical`] or [`FocusScope::Horizontal`].
    #[must_use]
    pub fn focus_scope(mut self, scope: FocusScope) -> Self {
        self.focus_scope = Some(scope);
        self
    }

    /// Paints and hits this element above the tree, outside ancestor overflow.
    ///
    /// The element leaves its parent's flex or grid, so the parent does not grow to fit it.
    /// Automatic width and height shrink to the content. A percentage resolves against the
    /// viewport. [`place`](Self::place) sets the top-left in viewport pixels; the default is the
    /// origin. A wheel over the element does not scroll the page underneath. A later overlay
    /// paints and hits above an earlier one, and a nested overlay paints above the one it sits in.
    #[must_use]
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    /// Top-left of an [`overlay`](Self::overlay), in viewport logical pixels.
    ///
    /// A [`Point`](cappy_layout::Point) stays where it is. A [`Dynamic`] point is read again when
    /// a signal it reads changes, and when a geometry key it reads changes. On an element that is
    /// not an overlay, the point is kept and not used.
    #[must_use]
    pub fn place(mut self, at: impl IntoPlace) -> Self {
        at.apply_place(&mut self);
        self
    }

    /// Key press or release while this element is focused. The clipboard is the one installed
    /// on the [`Ui`](crate::Ui).
    #[must_use]
    pub fn on_key(mut self, handler: impl FnMut(&KeyEvent, &mut dyn Clipboard) + 'static) -> Self {
        self.focusable = true;
        self.on_key = Some(Box::new(handler));
        self
    }

    /// Committed text while this element is focused. Shortcuts arrive through [`on_key`](Self::on_key).
    #[must_use]
    pub fn on_text(mut self, handler: impl FnMut(&str) + 'static) -> Self {
        self.focusable = true;
        self.on_text = Some(Box::new(handler));
        self
    }

    /// IME composition while this element is focused.
    #[must_use]
    pub fn on_ime(mut self, handler: impl FnMut(&ImeEvent) + 'static) -> Self {
        self.focusable = true;
        self.on_ime = Some(Box::new(handler));
        self
    }

    /// Called with `true` when this element gains focus and `false` when it loses it.
    #[must_use]
    pub fn on_focus(mut self, handler: impl FnMut(bool) + 'static) -> Self {
        self.on_focus = Some(Box::new(handler));
        self
    }

    /// Pointer down, move and up while this element is the pointer target. `text_index` is set
    /// when the pointer is over a text leaf.
    ///
    /// The primary button also focuses this element. The secondary and middle buttons are
    /// delivered here and do not focus, blur, or click. A context menu reads
    /// [`PointerButton::Secondary`](crate::PointerButton::Secondary) on
    /// [`PointerPhase::Down`](crate::PointerPhase::Down).
    #[must_use]
    pub fn on_pointer(mut self, handler: impl FnMut(&PointerEvent) + 'static) -> Self {
        self.focusable = true;
        self.on_pointer = Some(Box::new(handler));
        self
    }

    /// Called after a primary press that landed outside this element, once that press's click
    /// has run.
    ///
    /// Closing a menu here does not drop the item that was pressed. A press that starts inside
    /// this element, including on a nested overlay, does not dismiss it. A menu opened by the
    /// same click is not dismissed, because it was not mounted when the press began.
    #[must_use]
    pub fn on_dismiss(mut self, handler: impl FnMut(&ClickEvent) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }

    /// Caret byte index drawn on a text leaf. A [`Dynamic`] index is read again when its signals change.
    #[must_use]
    pub fn caret(mut self, index: impl IntoCaret) -> Self {
        index.add_caret(&mut self);
        self
    }

    /// Selection byte range drawn on a text leaf.
    #[must_use]
    pub fn selection(mut self, range: impl IntoSelection) -> Self {
        range.add_selection(&mut self);
        self
    }

    /// Replaces the input's value. A string is static. A [`Dynamic`] value (`:value` in `element!`)
    /// is read again when it changes. A [`Signal`] or [`Reactive`] string returned from that closure
    /// is written on each edit.
    ///
    /// # Panics
    /// Panics when the element is not an input.
    #[must_use]
    pub fn value(mut self, value: impl IntoInputValue) -> Self {
        self.input_spec().value = value.into_input_value();
        self
    }

    /// Text shown while the input is empty and not composing.
    ///
    /// # Panics
    /// Panics when the element is not an input.
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.input_spec().placeholder = text.into();
        self
    }

    /// Called with the new value after an edit.
    ///
    /// # Panics
    /// Panics when the element is not an input.
    #[must_use]
    pub fn on_input(mut self, handler: impl FnMut(&str) + 'static) -> Self {
        self.input_spec().on_input = Some(Box::new(handler));
        self
    }

    /// Called with the value when Enter is pressed. Enter does not insert a newline.
    ///
    /// # Panics
    /// Panics when the element is not an input.
    #[must_use]
    pub fn on_submit(mut self, handler: impl FnMut(&str) + 'static) -> Self {
        self.input_spec().on_submit = Some(Box::new(handler));
        self
    }

    /// A disabled node does not hover, press, focus, or show the pointer cursor. An input also
    /// does not edit. A [`Dynamic`](crate::Dynamic) flag is read again when a signal it reads
    /// changes.
    #[must_use]
    pub fn disabled(mut self, disabled: impl IntoDisabled) -> Self {
        disabled.apply_disabled(&mut self);
        self
    }

    /// Disables this node whenever `disabled` is true.
    #[must_use]
    pub fn disabled_when(self, disabled: impl Fn() -> bool + 'static) -> Self {
        self.disabled(Dynamic(disabled))
    }

    fn input_spec(&mut self) -> &mut InputSpec {
        let ElementContent::Input(spec) = &mut self.content else {
            panic!("this method is only for input elements");
        };
        spec
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
            !matches!(
                self.content,
                ElementContent::Image { .. } | ElementContent::Input(_)
            ),
            "image and input elements cannot have children"
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
            ElementContent::Input(_) => "input".to_owned(),
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
/// there. The `element!` builders for [`div`], [`text`], [`image`] and [`input`] do not: they call
/// those functions.
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
#[doc(hidden)]
pub struct DivBuilder {
    on_click: Option<ClickHandler>,
    on_hover: Option<HoverHandler>,
    on_dismiss: Option<ClickHandler>,
    stylesheet: Option<Arc<Stylesheet>>,
    disabled: Option<Box<dyn FnOnce(&mut Element)>>,
    overlay: bool,
    place: Option<Box<dyn FnOnce(&mut Element)>>,
    focus_scope: Option<FocusScope>,
}

impl DivBuilder {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self {
            on_click: None,
            on_hover: None,
            on_dismiss: None,
            stylesheet: None,
            disabled: None,
            overlay: false,
            place: None,
            focus_scope: None,
        }
    }

    /// Same scope as [`Element::focus_scope`].
    #[must_use]
    pub fn focus_scope(mut self, scope: FocusScope) -> Self {
        self.focus_scope = Some(scope);
        self
    }

    #[must_use]
    pub fn on_click(mut self, handler: impl FnMut(&ClickEvent) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    #[must_use]
    pub fn on_dismiss(mut self, handler: impl FnMut(&ClickEvent) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(handler));
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

    #[must_use]
    pub fn disabled(mut self, disabled: impl IntoDisabled + 'static) -> Self {
        self.disabled = Some(Box::new(move |element| disabled.apply_disabled(element)));
        self
    }

    #[must_use]
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    #[must_use]
    pub fn place(mut self, at: impl IntoPlace + 'static) -> Self {
        self.place = Some(Box::new(move |element| at.apply_place(element)));
        self
    }
}

impl IntoElement for DivBuilder {
    fn into_element(self) -> Element {
        let mut element = div();
        element.on_click = self.on_click;
        element.on_hover = self.on_hover;
        element.on_dismiss = self.on_dismiss;
        element.stylesheet = self.stylesheet;
        element.overlay = self.overlay;
        element.focus_scope = self.focus_scope;
        if let Some(apply) = self.disabled {
            apply(&mut element);
        }
        if let Some(apply) = self.place {
            apply(&mut element);
        }
        element
    }
}

/// `element!` `text` builder. [`open`](TextBuilder::open) has no content yet, so only
/// [`TextBuilder::content`] implements [`IntoElement`].
#[doc(hidden)]
pub struct TextBuilder<T> {
    content: T,
}

impl TextBuilder<()> {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self { content: () }
    }
}

impl<T> TextBuilder<T> {
    #[must_use]
    pub fn content<U>(self, content: U) -> TextBuilder<U> {
        TextBuilder { content }
    }

    /// Same setter as [`content`](Self::content). `value = "..."` is static. `:value = ...` is
    /// dynamic: a string expression, or a [`Signal`] / [`Reactive`] string.
    #[must_use]
    pub fn value<U>(self, value: U) -> TextBuilder<U> {
        TextBuilder { content: value }
    }
}

impl<T: IntoText> IntoElement for TextBuilder<T> {
    fn into_element(self) -> Element {
        text(self.content)
    }
}

/// `element!` `image` builder. [`open`](ImageBuilder::open) has no pixels yet, so only
/// [`ImageBuilder::content`] implements [`IntoElement`].
#[doc(hidden)]
pub struct ImageBuilder<T> {
    content: T,
    fit: ObjectFit,
}

impl ImageBuilder<()> {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self {
            content: (),
            fit: ObjectFit::Fill,
        }
    }
}

impl<T> ImageBuilder<T> {
    #[must_use]
    pub fn content<U>(self, content: U) -> ImageBuilder<U> {
        ImageBuilder {
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

impl<T: Into<ImageSource>> IntoElement for ImageBuilder<T> {
    fn into_element(self) -> Element {
        image(self.content).fit(self.fit)
    }
}

/// `element!` `input` builder. [`InputBuilder::open`] starts empty; [`InputBuilder::value`]
/// replaces that.
#[doc(hidden)]
pub struct InputBuilder<T> {
    value: T,
    placeholder: String,
    on_input: Option<InputHandler>,
    on_submit: Option<InputHandler>,
    disabled: bool,
}

impl InputBuilder<()> {
    #[must_use]
    pub fn open(_cx: &crate::Context) -> Self {
        Self {
            value: (),
            placeholder: String::new(),
            on_input: None,
            on_submit: None,
            disabled: false,
        }
    }
}

impl<T> InputBuilder<T> {
    #[must_use]
    pub fn value<U>(self, value: U) -> InputBuilder<U> {
        InputBuilder {
            value,
            placeholder: self.placeholder,
            on_input: self.on_input,
            on_submit: self.on_submit,
            disabled: self.disabled,
        }
    }

    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    #[must_use]
    pub fn on_input(mut self, handler: impl FnMut(&str) + 'static) -> Self {
        self.on_input = Some(Box::new(handler));
        self
    }

    #[must_use]
    pub fn on_submit(mut self, handler: impl FnMut(&str) + 'static) -> Self {
        self.on_submit = Some(Box::new(handler));
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl IntoElement for InputBuilder<()> {
    fn into_element(self) -> Element {
        finish_input(self, String::new())
    }
}

impl<T: IntoInputValue> IntoElement for InputBuilder<T> {
    fn into_element(self) -> Element {
        let value = self.value.into_input_value();
        let mut element = input();
        element.input_spec().value = value;
        apply_input_fields(
            &mut element,
            self.placeholder,
            self.on_input,
            self.on_submit,
            self.disabled,
        );
        element
    }
}

fn finish_input(builder: InputBuilder<()>, value: String) -> Element {
    let mut element = input().value(value);
    apply_input_fields(
        &mut element,
        builder.placeholder,
        builder.on_input,
        builder.on_submit,
        builder.disabled,
    );
    element
}

fn apply_input_fields(
    element: &mut Element,
    placeholder: String,
    on_input: Option<InputHandler>,
    on_submit: Option<InputHandler>,
    disabled: bool,
) {
    let spec = element.input_spec();
    spec.placeholder = placeholder;
    spec.on_input = on_input;
    spec.on_submit = on_submit;
    spec.disabled = disabled;
    element.disabled = disabled;
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
        let element = DivBuilder::open(&crate::Application::new().context()).into_element();
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
