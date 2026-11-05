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
/// use cappy_core::{Space, div, signal, text};
///
/// let count = signal(0);
/// let tree = ui! {
///     div(class = "app") {
///         text("Counter", class = "title"),
///         div(class = "row", style = style! { gap: Space::Sm }) {
///             text(:format!("Count: {}", count.get()), key = "label"),
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
/// - `name(content, field = value, ...) { children }` calls the function `name` in scope, so
///   `div`, `text` and your own element functions all resolve (and go to definition) the same
///   way. The leading `content` argument, the fields and the children are all optional.
/// - `@Type(field = value, ...)` builds `Type::default()` and sets fields, e.g. a struct using
///   `#[derive(Props)]` that implements [`Component`](cappy_core::Component).
/// - `{ expression }` embeds an element; `{ ..iterator }` embeds every element it yields.
///
/// Every `field = value` calls the builder method `field(value)`. A leading `:` makes the value
/// reactive: `:field = value` calls `field(Dynamic(move || value))` and the content `:value`
/// calls `name(Dynamic(move || value))`, so the value is recomputed whenever a signal it reads
/// changes. The setter must accept [`Dynamic`](cappy_core::Dynamic), as `text`, `class` and
/// `style` do.
///
/// `class` takes one class or a list: `class = [a, b]` is shorthand for `class = a, class = b`,
/// and `:class = [a, b]` for `:class = a, :class = b`. List items may mix strings and typed
/// classes.
///
/// Children may be separated by commas. Put a comma before a `{ expression }` child that follows
/// an element with arguments, or it is read as that element's children.
#[macro_export]
macro_rules! ui {
    ($($node:tt)+) => {
        $crate::__ui_node!($($node)+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_node {
    (@ $type:ident $(:: $segment:ident)* $(( $($fields:tt)* ))? $(,)?) => {
        $crate::__private::Element::from(
            $crate::__ui_args!(@fields [<$type $(:: $segment)*>::default()] $($($fields)*)?)
        )
    };
    ({ $element:expr } $(,)?) => {
        $crate::__private::Element::from($element)
    };
    ($name:ident $(( $($args:tt)* ))? $({ $($children:tt)* })? $(,)?) => {
        $crate::__ui_children!(
            $crate::__ui_args!(@head $name ; $($($args)*)?) ; $($($children)*)?
        )
    };
}

/// Turns an element's argument list into a builder chain. `@head` picks the constructor call
/// from the optional leading content; `@fields` appends one `field = value` per step.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_args {
    (@head $name:ident ;) => {
        $name()
    };
    (@head $name:ident ; : $field:ident = $($rest:tt)*) => {
        $crate::__ui_args!(@fields [$name()] : $field = $($rest)*)
    };
    (@head $name:ident ; $field:ident = $($rest:tt)*) => {
        $crate::__ui_args!(@fields [$name()] $field = $($rest)*)
    };
    (@head $name:ident ; : $content:expr $(, $($rest:tt)*)?) => {
        $crate::__ui_args!(
            @fields [$crate::__ui_bind!(content $name, $content)] $($($rest)*)?
        )
    };
    (@head $name:ident ; $content:expr $(, $($rest:tt)*)?) => {
        $crate::__ui_args!(@fields [$name($content)] $($($rest)*)?)
    };
    (@fields [$($builder:tt)*]) => {
        $($builder)*
    };
    (@fields [$($builder:tt)*] class = [$($class:expr),* $(,)?] $(, $($rest:tt)*)?) => {
        $crate::__ui_args!(@fields [$($builder)*] $(class = $class,)* $($($rest)*)?)
    };
    (@fields [$($builder:tt)*] : class = [$($class:expr),* $(,)?] $(, $($rest:tt)*)?) => {
        $crate::__ui_args!(@fields [$($builder)*] $(: class = $class,)* $($($rest)*)?)
    };
    (@fields [$($builder:tt)*] : $field:ident = $value:expr $(, $($rest:tt)*)?) => {
        $crate::__ui_args!(
            @fields [$crate::__ui_bind!(field $($builder)*, $field = $value)] $($($rest)*)?
        )
    };
    (@fields [$($builder:tt)*] $field:ident = $value:expr $(, $($rest:tt)*)?) => {
        $crate::__ui_args!(@fields [$($builder)*.$field($value)] $($($rest)*)?)
    };
}

/// Expands the dynamic parts of `ui!`, the ones written with a leading `:`: the value becomes a
/// [`Dynamic`](cappy_core::Dynamic) closure passed to the same function or setter a static value
/// would go to, so go to definition lands on that function.
///
/// - `content`: `name(:value, ...)` calls `name(Dynamic(move || value))`.
/// - `field`: `:field = value` calls `.field(Dynamic(move || value))` on the builder so far.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_bind {
    (content $name:ident, $value:expr) => {
        $name($crate::__private::Dynamic(move || $value))
    };
    (field $builder:expr, $field:ident = $value:expr) => {
        $builder.$field($crate::__private::Dynamic(move || $value))
    };
}

/// Appends child nodes to `$parent`, one node per step.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_children {
    ($parent:expr ;) => {
        $parent
    };
    ($parent:expr ; , $($rest:tt)*) => {
        $crate::__ui_children!($parent ; $($rest)*)
    };
    ($parent:expr ; { .. $children:expr } $($rest:tt)*) => {
        $crate::__ui_children!($parent.children($children) ; $($rest)*)
    };
    ($parent:expr ; { $child:expr } $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($child) ; $($rest)*)
    };
    ($parent:expr ; @ $type:ident $(:: $segment:ident)* ( $($props:tt)* ) $($rest:tt)*) => {
        $crate::__ui_children!(
            $parent.child($crate::__ui_node!(@ $type $(:: $segment)* ( $($props)* ))) ; $($rest)*
        )
    };
    ($parent:expr ; @ $type:ident $(:: $segment:ident)* $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!(@ $type $(:: $segment)*)) ; $($rest)*)
    };
    ($parent:expr ; $name:ident ( $($args:tt)* ) { $($children:tt)* } $($rest:tt)*) => {
        $crate::__ui_children!(
            $parent.child($crate::__ui_node!($name ( $($args)* ) { $($children)* })) ; $($rest)*
        )
    };
    ($parent:expr ; $name:ident ( $($args:tt)* ) $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!($name ( $($args)* ))) ; $($rest)*)
    };
    ($parent:expr ; $name:ident { $($children:tt)* } $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!($name { $($children)* })) ; $($rest)*)
    };
    ($parent:expr ; $name:ident $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($crate::__ui_node!($name)) ; $($rest)*)
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
