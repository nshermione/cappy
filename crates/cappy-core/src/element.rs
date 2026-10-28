//! Declarative description of a UI tree. An [`Element`] is mounted into a [`crate::Ui`] once;
//! afterwards only its reactive bindings re-run.

use std::fmt;
use std::sync::Arc;

use cappy_style::{ClassName, StyleRefinement};

use crate::ClickEvent;

pub(crate) type ClickHandler = Box<dyn FnMut(&ClickEvent)>;
pub(crate) type HoverHandler = Box<dyn FnMut(bool)>;
pub(crate) type TextBinding = Box<dyn Fn() -> String>;
pub(crate) type ClassBinding = Box<dyn Fn() -> Vec<ClassName>>;
pub(crate) type StyleBinding = Box<dyn Fn() -> StyleRefinement>;

pub(crate) enum ElementContent {
    Container,
    StaticText(Arc<str>),
    DynamicText(TextBinding),
}

/// A node description: a container (`div`) or a text leaf, with classes, style and handlers.
pub struct Element {
    pub(crate) content: ElementContent,
    pub(crate) key: Option<String>,
    pub(crate) classes: Vec<ClassName>,
    pub(crate) class_binding: Option<ClassBinding>,
    pub(crate) style: StyleRefinement,
    pub(crate) style_binding: Option<StyleBinding>,
    pub(crate) on_click: Option<ClickHandler>,
    pub(crate) on_hover: Option<HoverHandler>,
    pub(crate) children: Vec<Element>,
}

/// A container element.
pub fn div() -> Element {
    Element::new(ElementContent::Container)
}

/// A text leaf whose content never changes.
pub fn text(content: impl Into<Arc<str>>) -> Element {
    Element::new(ElementContent::StaticText(content.into()))
}

/// A text leaf whose content is recomputed whenever a signal it reads changes.
pub fn text_dyn(content: impl Fn() -> String + 'static) -> Element {
    Element::new(ElementContent::DynamicText(Box::new(content)))
}

impl Element {
    fn new(content: ElementContent) -> Self {
        Self {
            content,
            key: None,
            classes: Vec::new(),
            class_binding: None,
            style: StyleRefinement::default(),
            style_binding: None,
            on_click: None,
            on_hover: None,
            children: Vec::new(),
        }
    }

    pub fn is_text(&self) -> bool {
        !matches!(self.content, ElementContent::Container)
    }

    /// Adds classes from a space-separated list, e.g. `"btn btn-primary"`, or a single typed
    /// class from `#[derive(ClassName)]`.
    #[must_use]
    pub fn class(mut self, list: impl AsRef<str>) -> Self {
        self.classes.extend(ClassName::parse_list(list.as_ref()));
        self
    }

    /// Classes recomputed whenever a signal they read changes. Applied after static classes.
    #[must_use]
    pub fn class_dyn(mut self, classes: impl Fn() -> String + 'static) -> Self {
        self.class_binding = Some(Box::new(move || ClassName::parse_list(&classes())));
        self
    }

    /// Inline style, layered over the classes.
    #[must_use]
    pub fn style(mut self, refinement: StyleRefinement) -> Self {
        self.style.merge(&refinement);
        self
    }

    /// Inline style recomputed whenever a signal it reads changes. Layered over `style`.
    #[must_use]
    pub fn style_dyn(mut self, refinement: impl Fn() -> StyleRefinement + 'static) -> Self {
        self.style_binding = Some(Box::new(refinement));
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
}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.content {
            ElementContent::Container => "div".to_owned(),
            ElementContent::StaticText(text) => format!("text({text:?})"),
            ElementContent::DynamicText(_) => "text(<dynamic>)".to_owned(),
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
pub trait Component {
    fn render(self) -> Element;
}

impl<C: Component> From<C> for Element {
    fn from(component: C) -> Self {
        component.render()
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
}
