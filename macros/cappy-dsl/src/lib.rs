//! Declarative macros for cappy: [`ui!`] builds element trees, [`style!`] builds style
//! refinements and [`theme!`] derives themes, all expanding to plain builder calls.

#[doc(hidden)]
pub mod __private {
    pub use cappy_core::*;
}

/// Builds an element tree.
///
/// ```
/// use cappy_dsl::{style, ui};
/// use cappy_core::{Space, signal};
///
/// let count = signal(0);
/// let tree = ui! {
///     div(class = "app") {
///         text("Counter", class = "title")
///         div(class = "row", style = style! { gap: Space::Sm }) {
///             text(move || format!("Count: {}", count.get()), key = "label")
///             div(class = "btn btn-primary", on_click = move |_| count.update(|n| *n += 1)) {
///                 text("+1")
///             }
///         }
///     }
/// };
/// # let _ = tree;
/// ```
///
/// Nodes:
/// - `div(attr = value, ...) { children }`; the attributes and the children are both optional.
/// - `text(content, attr = value, ...)`; a closure such as `move || ...` makes it reactive.
/// - `name(attr = value, ...) { children }` calls a function `name()` that returns an
///   [`Element`](cappy_core::Element).
/// - `@Type(prop = value, ...)` builds `Type::default()` and sets props, e.g. a struct using
///   `#[derive(Props)]` that implements [`Component`](cappy_core::Component).
/// - `{ expression }` embeds an element; `{ ..iterator }` embeds every element it yields.
///
/// Every `attr = value` calls the builder method `attr(value)`.
#[macro_export]
macro_rules! ui {
    ($($node:tt)+) => {
        $crate::__ui_node!($($node)+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_node {
    (text ( move || $body:expr $(, $key:ident = $value:expr)* $(,)? )) => {
        $crate::__private::text_dyn(move || $body)$(.$key($value))*
    };
    (text ( || $body:expr $(, $key:ident = $value:expr)* $(,)? )) => {
        $crate::__private::text_dyn(|| $body)$(.$key($value))*
    };
    (text ( $content:expr $(, $key:ident = $value:expr)* $(,)? )) => {
        $crate::__private::text($content)$(.$key($value))*
    };
    (div $(( $($key:ident = $value:expr),* $(,)? ))? $({ $($children:tt)* })?) => {
        $crate::__ui_children!(
            $crate::__private::div()$($(.$key($value))*)? ; $($($children)*)?
        )
    };
    (@ $type:ident $(:: $segment:ident)* $(( $($key:ident = $value:expr),* $(,)? ))?) => {
        $crate::__private::Element::from(
            <$type $(:: $segment)*>::default()$($(.$key($value))*)?
        )
    };
    ({ $element:expr }) => {
        $crate::__private::Element::from($element)
    };
    ($name:ident $(( $($key:ident = $value:expr),* $(,)? ))? $({ $($children:tt)* })?) => {
        $crate::__ui_children!($name()$($(.$key($value))*)? ; $($($children)*)?)
    };
}

/// Appends child nodes to `$parent`, one node per step.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_children {
    ($parent:expr ;) => {
        $parent
    };
    ($parent:expr ; { .. $children:expr } $($rest:tt)*) => {
        $crate::__ui_children!($parent.children($children) ; $($rest)*)
    };
    ($parent:expr ; { $child:expr } $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($child) ; $($rest)*)
    };
    ($parent:expr ; text ( $($args:tt)* ) $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!(text ( $($args)* ))) ; $($rest)*)
    };
    ($parent:expr ; @ $type:ident $(:: $segment:ident)* ( $($props:tt)* ) $($rest:tt)*) => {
        $crate::__ui_children!(
            $parent.child($crate::__ui_node!(@ $type $(:: $segment)* ( $($props)* ))) ; $($rest)*
        )
    };
    ($parent:expr ; @ $type:ident $(:: $segment:ident)* $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!(@ $type $(:: $segment)*)) ; $($rest)*)
    };
    ($parent:expr ; $name:ident ( $($attrs:tt)* ) { $($children:tt)* } $($rest:tt)*) => {
        $crate::__ui_children!(
            $parent.child($crate::__ui_node!($name ( $($attrs)* ) { $($children)* })) ; $($rest)*
        )
    };
    ($parent:expr ; $name:ident ( $($attrs:tt)* ) $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!($name ( $($attrs)* ))) ; $($rest)*)
    };
    ($parent:expr ; $name:ident { $($children:tt)* } $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!($name { $($children)* })) ; $($rest)*)
    };
}

/// Builds a [`StyleRefinement`](cappy_core::StyleRefinement): each `key: value` calls the
/// setter `key(value)`.
///
/// ```
/// use cappy_dsl::style;
/// use cappy_core::{ColorToken, Radius, Space};
///
/// let card = style! {
///     padding: Space::Lg,
///     background: ColorToken::Surface,
///     radius: Radius::Md,
///     width: 240,
/// };
/// # let _ = card;
/// ```
#[macro_export]
macro_rules! style {
    ($($key:ident : $value:expr),* $(,)?) => {
        $crate::__private::StyleRefinement::default()$(.$key($value))*
    };
}

/// Builds a [`Theme`](cappy_core::Theme) from a base theme plus overrides. Layout is never part
/// of a theme, so switching themes restyles an app without changing its structure.
///
/// Sections are optional but must appear in this order. Colors are hex literals, checked at
/// compile time.
///
/// ```
/// use cappy_dsl::{style, theme};
/// use cappy_core::{ColorToken, Theme};
///
/// let ocean = theme! {
///     name: "ocean",
///     extends: Theme::dark(),
///     colors { primary: "#0ea5e9", primary_hover: "#38bdf8" },
///     spacing { md: 10.0 },
///     radius { md: 8.0 },
///     font_sizes { md: 15.0 },
///     line_height: 1.5,
///     classes {
///         "chip" => style! { background: ColorToken::Primary },
///         "chip:hover" => style! { background: ColorToken::PrimaryHover },
///     },
/// };
/// assert_eq!(ocean.name(), "ocean");
/// assert!(ocean.has_class("chip"));
/// ```
///
/// An invalid color does not compile:
///
/// ```compile_fail
/// let broken = cappy_dsl::theme! { colors { primary: "#12345" } };
/// ```
#[macro_export]
macro_rules! theme {
    (
        $(name: $name:expr,)?
        $(extends: $base:expr,)?
        $(colors { $($color:ident : $hex:literal),* $(,)? } $(,)?)?
        $(spacing { $($space:ident : $space_value:expr),* $(,)? } $(,)?)?
        $(radius { $($radius:ident : $radius_value:expr),* $(,)? } $(,)?)?
        $(font_sizes { $($font:ident : $font_value:expr),* $(,)? } $(,)?)?
        $(line_height: $line_height:expr,)?
        $(classes { $($selector:literal => $class:expr),* $(,)? } $(,)?)?
    ) => {{
        let mut theme: $crate::__private::Theme = $crate::__theme_base!($($base)?);
        $(theme.set_name($name);)?
        $($(theme.tokens_mut().colors.$color = $crate::__private::hex!($hex);)*)?
        $($(theme.tokens_mut().spacing.$space = $space_value;)*)?
        $($(theme.tokens_mut().radius.$radius = $radius_value;)*)?
        $($(theme.tokens_mut().font_sizes.$font = $font_value;)*)?
        $(theme.tokens_mut().line_height = $line_height;)?
        $($(theme.set_class($selector, $class);)*)?
        theme
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __theme_base {
    () => {
        $crate::__private::Theme::light()
    };
    ($base:expr) => {
        $base
    };
}
