//! Values an element setter accepts either fixed or recomputed from signals. [`Dynamic`] marks the
//! recomputed kind; `element!` wraps every `:field = value` and `name(:value)` in it, so a setter keeps
//! one name for both.

use std::borrow::Cow;
use std::sync::Arc;

use cappy_style::{ClassName, StyleRefinement};

use crate::element::{ClassBinding, Element, ElementContent, StyleBinding};
use crate::reactive::{Reactive, Signal};

/// A value recomputed by calling the closure again whenever a signal it read changes.
pub struct Dynamic<F>(pub F);

/// What [`Dynamic`] text and input values may return. A string is read as itself. A [`Signal`] or
/// [`Reactive`] string is read with [`Signal::get`], so the closure does not have to call it.
pub trait LiveText {
    fn read_text(&self) -> String;

    /// Writes an edit back. Strings ignore it. A [`Signal`] or [`Reactive`] string stores it.
    fn write_text(&self, value: String) {
        let _ = value;
    }
}

impl LiveText for String {
    fn read_text(&self) -> String {
        self.clone()
    }
}

impl LiveText for &str {
    fn read_text(&self) -> String {
        (*self).to_owned()
    }
}

impl LiveText for Arc<str> {
    fn read_text(&self) -> String {
        self.to_string()
    }
}

impl LiveText for Cow<'static, str> {
    fn read_text(&self) -> String {
        self.to_string()
    }
}

impl LiveText for Signal<String> {
    fn read_text(&self) -> String {
        self.get()
    }

    fn write_text(&self, value: String) {
        self.set(value);
    }
}

impl LiveText for Reactive<String> {
    fn read_text(&self) -> String {
        self.get()
    }

    fn write_text(&self, value: String) {
        self.set(value);
    }
}

/// Content for [`text`](crate::text): a string, or a [`Dynamic`] closure returning one.
pub trait IntoText {
    fn into_text(self) -> Element;
}

impl<T: Into<Arc<str>>> IntoText for T {
    fn into_text(self) -> Element {
        Element::new(ElementContent::StaticText(self.into()))
    }
}

impl<F, R> IntoText for Dynamic<F>
where
    F: Fn() -> R + 'static,
    R: LiveText,
{
    fn into_text(self) -> Element {
        let Dynamic(content) = self;
        Element::new(ElementContent::DynamicText(Box::new(move || {
            content().read_text()
        })))
    }
}

/// Classes for [`Element::class`]: a space-separated list or a typed class, or a [`Dynamic`]
/// closure returning one.
pub trait IntoClasses {
    fn add_to(self, element: &mut Element);
}

impl<T: AsRef<str>> IntoClasses for T {
    fn add_to(self, element: &mut Element) {
        element.classes.extend(ClassName::parse_list(self.as_ref()));
    }
}

impl<F, R> IntoClasses for Dynamic<F>
where
    F: Fn() -> R + 'static,
    R: AsRef<str>,
{
    fn add_to(self, element: &mut Element) {
        let Dynamic(classes) = self;
        let current = move || ClassName::parse_list(classes().as_ref());
        let binding: ClassBinding = match element.class_binding.take() {
            Some(previous) => Box::new(move || {
                let mut names = previous();
                names.extend(current());
                names
            }),
            None => Box::new(current),
        };
        element.class_binding = Some(binding);
    }
}

/// Inline style for [`Element::style`]: a [`StyleRefinement`], or a [`Dynamic`] closure returning
/// one.
pub trait IntoStyle {
    fn add_to(self, element: &mut Element);
}

impl IntoStyle for StyleRefinement {
    fn add_to(self, element: &mut Element) {
        element.style.merge(&self);
    }
}

impl<F> IntoStyle for Dynamic<F>
where
    F: Fn() -> StyleRefinement + 'static,
{
    fn add_to(self, element: &mut Element) {
        let Dynamic(current) = self;
        let binding: StyleBinding = match element.style_binding.take() {
            Some(previous) => Box::new(move || {
                let mut refinement = previous();
                refinement.merge(&current());
                refinement
            }),
            None => Box::new(current),
        };
        element.style_binding = Some(binding);
    }
}
