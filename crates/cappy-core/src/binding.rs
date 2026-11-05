//! Values an element setter accepts either fixed or recomputed from signals. [`Dynamic`] marks the
//! recomputed kind; `ui!` wraps every `:field = value` and `name(:value)` in it, so a setter keeps
//! one name for both.

use std::sync::Arc;

use cappy_style::{ClassName, StyleRefinement};

use crate::element::{ClassBinding, Element, ElementContent, StyleBinding};

/// A value recomputed by calling the closure again whenever a signal it read changes.
pub struct Dynamic<F>(pub F);

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
    R: Into<String>,
{
    fn into_text(self) -> Element {
        let Dynamic(content) = self;
        Element::new(ElementContent::DynamicText(Box::new(move || {
            content().into()
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
