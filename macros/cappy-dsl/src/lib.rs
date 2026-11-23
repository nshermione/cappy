//! Declarative macros for cappy: [`element!`] builds element trees and [`style!`] builds style
//! refinements, both expanding to plain builder calls.

#[doc(hidden)]
pub mod __private {
    pub use cappy_core::*;
}

#[doc(hidden)]
pub use cappy_macros::{slot_self, ui_closure};

/// Builds an element tree.
///
/// ```
/// use cappy_dsl::{style, element};
/// use cappy_core::{Application, Space, div, text};
///
/// let _application = Application::new();
/// let count = cappy_core::Context::current().reactive(0_i32);
/// let tree = element! {
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
/// - `Name(field = value, ...) { children }` builds `Name::open()` and sets fields.
///   `div`, `text`, `image` and `input` are that path for the built-in nodes. A leading value is
///   `content`, so `text("Title")` calls `.content("Title")`. `class`, `style` and `key` are
///   applied to the element `into_element` returns. Other fields are setters on the builder.
///   Without slots, children are appended. With slots, the first child that has no `slot = name`
///   fills the first slot, `slot = name` fills that slot, and further unnamed children are dropped.
/// - `slot(field)` inside a component's `render` places that `#[slot]` field. It reads
///   `self.field`.
/// - `{ expression }` embeds an element; `{ ..iterator }` embeds every element it yields. A
///   function that returns an element is written here, not as `name(...)`.
/// - `show (:cond) { ... }` keeps its nodes and hides them while `cond` is false.
/// - `if (:cond) { ... } else { ... }` disposes the branch that is not selected, including
///   nested `show`, `if` and `for`. `else` may be omitted or written `else if`.
/// - `for (item in :list, key = item.field) { ... }` repeats the block, keeping a row when its
///   key stays. `item` in the block is the row signal.
/// - `show`, `if` and `for` build their elements in a `'static` closure. Signals those closures
///   read belong to the runtime [`Application::new`](cappy_core::Application::new) installed on
///   this thread.
/// - `slot(field)` stays outside `show`, `if` and `for`. A parent fills slots before mount, and
///   those blocks build their contents at mount.
///
/// `element! { div() }` before [`Application::new`](cappy_core::Application::new) panics when a
/// node allocates a signal. Builders that allocate nothing do not.
///
/// Every `field = value` calls the builder method `field(value)`. A leading `:` makes the value
/// reactive: `:field = value` calls `field(Dynamic(move || value))` and the content `:value`
/// calls `name(Dynamic(move || value))`, so the value is recomputed whenever a signal it reads
/// changes. `self.field` inside that expression, and inside `show`, `if` and `for`, is copied
/// out before the closure. The setter must accept [`Dynamic`](cappy_core::Dynamic), as `text`, `class`
/// and `style` do.
///
/// `text` and `input` share `value`. `value = "hello"` is static. `:value = "hello"` or
/// `:value = name`, where `name` is a [`Signal`](cappy_core::Signal) or
/// [`Reactive`](cappy_core::Reactive) string, is dynamic. The signal is read for you. An input
/// writes edits back to that signal.
///
/// `class` takes one class or a list: `class = [a, b]` is shorthand for `class = a, class = b`,
/// and `:class = [a, b]` for `:class = a, :class = b`. List items may mix strings and typed
/// classes.
///
/// Children may be separated by commas. Put a comma before a `{ expression }` child that follows
/// an element with arguments, or it is read as that element's children.
#[macro_export]
macro_rules! element {
    ($($body:tt)+) => {
        $crate::__ui_node!($($body)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_node {
    (show ( : $cond:expr ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_show_expr!(: $cond ; $($body)*)
    };
    (show ( $cond:expr ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_show_expr!(: $cond ; $($body)*)
    };
    (if ( : $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_root!(
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(move || $crate::__ui_fragment!($($then)*)) ;
            $($rest)*
        )
    };
    (if ( $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_root!(
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(move || $crate::__ui_fragment!($($then)*)) ;
            $($rest)*
        )
    };
    (for ( $item:ident in : $source:expr , key = $key:ident . $($field:ident).+ ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_for_expr!($item ; $source ; $key ; path $($field).+ ; $($body)*)
    };
    (for ( $item:ident in : $source:expr , key = $key:ident ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_for_expr!($item ; $source ; $key ; self ; $($body)*)
    };
    ($ctor:ident ( $field:ident , ) { $($child_body:tt)* } $(,)?) => {
        $crate::__ui_slot_expr!($ctor ; $field ; [ $($child_body)* ])
    };
    ($ctor:ident ( $field:ident ) { $($child_body:tt)* } $(,)?) => {
        $crate::__ui_slot_expr!($ctor ; $field ; [ $($child_body)* ])
    };
    ($ctor:ident ( $field:ident , ) $(,)?) => {
        $crate::__ui_slot_expr!($ctor ; $field ; [])
    };
    ($ctor:ident ( $field:ident ) $(,)?) => {
        $crate::__ui_slot_expr!($ctor ; $field ; [])
    };
    ($type:ident $(:: $segment:ident)* $(( $($fields:tt)* ))? $({ $($children:tt)* })? $(,)?) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [] ;
            host [] ;
            children [ $($($children)*)? ] ;
            $($($fields)*)?
        )
    };
    ({ $element:expr } $(,)?) => {
        $crate::__private::IntoElement::into_element($element)
    };
}

/// Calls `Type::open`. `div`, `text`, `image` and `input` open the built-in builders. The value
/// parameter is inferred from `content` or `value`.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_open {
    (div) => {
        $crate::__private::DivBuilder::open()
    };
    (text) => {
        $crate::__private::TextBuilder::open()
    };
    (image) => {
        $crate::__private::ImageBuilder::open()
    };
    (input) => {
        $crate::__private::InputBuilder::open()
    };
    ($type:ident $(:: $segment:ident)+) => {
        <$type $(:: $segment)*>::open()
    };
    ($type:ident) => {
        $crate::__ui_open!(@name $type ; $type)
    };
    (@name $other:ident ; $name:ident) => {
        <$name>::open()
    };
}

/// Resolves `div`, `text` and `image` to their builders. Every other path is the component type.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_type_path {
    (div) => {
        $crate::__private::DivBuilder
    };
    (text) => {
        $crate::__private::TextBuilder
    };
    (image) => {
        $crate::__private::ImageBuilder
    };
    (input) => {
        $crate::__private::InputBuilder
    };
    ($type:ident $(:: $segment:ident)*) => {
        $type $(:: $segment)*
    };
}

/// Splits node arguments. `class`, `style` and `key` stay on the element `into_element` returns.
/// A leading value is the `content` setter. Every other field is a setter on the builder. Children
/// fill slots, or append to that root when it has none.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_component {
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        class = [ $($class:expr),* $(,)? ] $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* class = [ $($class),* ], ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : class = [ $($class:expr),* $(,)? ] $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : class = [ $($class),* ], ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        class = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* class = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : class = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : class = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        style = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* style = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : style = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : style = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        key = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* key = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : key = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : key = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : $field:ident = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* : $field = $value, ] ;
            host [ $($host)* ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        $field:ident = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* $field = $value, ] ;
            host [ $($host)* ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : $content:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* : content = $content, ] ;
            host [ $($host)* ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        $content:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            $type $(:: $segment)* ;
            props [ $($props)* content = $content, ] ;
            host [ $($host)* ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
    ) => {
        $crate::__ui_args!(
            @fields
            [
                $crate::__ui_slot_children!(
                    $crate::__private::IntoElement::into_element(
                        $crate::__ui_args!(@fields [$crate::__ui_open!($type $(:: $segment)*)] $($props)*)
                    ).prepare_slots() ;
                    $type $(:: $segment)* ;
                    $($children)*
                )
            ]
            $($host)*
        )
    };
}

/// `slot(field)` reads `self.field`. Any other single-ident call is that builder's `content`.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_slot_expr {
    ($ctor:ident ; $field:ident ; [ $($children:tt)* ]) => {
        $crate::__ui_slot_expr!(@is $ctor ; $field ; $ctor ; [ $($children)* ])
    };
    (@is slot ; $field:ident ; $ctor:ident ; [ $($children:tt)* ]) => {
        $ctor($crate::slot_self!($field))
    };
    (@is $other:ident ; $field:ident ; $ctor:ident ; [ $($children:tt)* ]) => {
        $crate::__ui_component!(
            $ctor ;
            props [ content = $field, ] ;
            host [] ;
            children [ $($children)* ] ;
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

/// Expands the dynamic parts of `element!`, the ones written with a leading `:`: the value becomes a
/// [`Dynamic`](cappy_core::Dynamic) closure passed to the same function or setter a static value
/// would go to, so go to definition lands on that function.
///
/// - `content`: `name(:value, ...)` calls `name(Dynamic(move || value))`.
/// - `field`: `:field = value` calls `.field(Dynamic(move || value))` on the builder so far.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_bind {
    (content $name:ident, $value:expr) => {
        $name($crate::__private::Dynamic($crate::ui_closure!(move || {
            $value
        })))
    };
    (field $builder:expr, $field:ident = $value:expr) => {
        $builder.$field($crate::__private::Dynamic($crate::ui_closure!(move || {
            $value
        })))
    };
}

/// Appends child nodes to `$parent`, one node per step.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_children {
    ($parent:expr ; $($body:tt)*) => {{
        let mut __cappy_nodes: ::std::vec::Vec<$crate::__private::Element> = ::std::vec::Vec::new();
        $crate::__ui_collect!(__cappy_nodes ; $($body)*);
        __cappy_nodes
            .into_iter()
            .fold($parent, |parent, child| parent.child(child))
    }};
}

/// Pushes each child element into `$nodes`.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_collect {
    ($nodes:ident ;) => {};
    ($nodes:ident ; , $($rest:tt)*) => {
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; { .. $children:expr } $($rest:tt)*) => {
        $nodes.extend($children.into_iter().map($crate::__private::IntoElement::into_element));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; { $child:expr } $($rest:tt)*) => {
        $nodes.push($crate::__private::IntoElement::into_element($child));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; show ( : $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_show_expr!(: $cond ; $($body)*));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; show ( $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_show_expr!(: $cond ; $($body)*));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; if ( : $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @start $nodes ;
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(move || $crate::__ui_fragment!($($then)*)) ;
            $($rest)*
        )
    };
    ($nodes:ident ; if ( $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @start $nodes ;
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(move || $crate::__ui_fragment!($($then)*)) ;
            $($rest)*
        )
    };
    ($nodes:ident ; for ( $item:ident in : $source:expr , key = $key:ident . $($field:ident).+ ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_for_expr!($item ; $source ; $key ; path $($field).+ ; $($body)*));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; for ( $item:ident in : $source:expr , key = $key:ident ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_for_expr!($item ; $source ; $key ; self ; $($body)*));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; $ctor:ident ( $field:ident ) { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!($ctor ( $field ) { $($children)* }));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; $ctor:ident ( $field:ident ) $($rest:tt)*) => {
        $crate::__ui_collect_slot!($nodes ; $ctor ; $field ; $($rest)*)
    };
    ($nodes:ident ; $name:ident $(:: $segment:ident)* ( $($args:tt)* ) { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!($name $(:: $segment)* ( $($args)* ) { $($children)* }));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; $name:ident $(:: $segment:ident)* ( $($args:tt)* ) $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!($name $(:: $segment)* ( $($args)* )));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; $name:ident $(:: $segment:ident)* { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!($name $(:: $segment)* { $($children)* }));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    ($nodes:ident ; $name:ident $(:: $segment:ident)* $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!($name $(:: $segment)*));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_collect_slot {
    ($nodes:ident ; $ctor:ident ; $field:ident ; $($rest:tt)*) => {
        $crate::__ui_collect_slot!(@is $nodes ; $ctor ; $field ; $ctor ; $($rest)*)
    };
    (@is $nodes:ident ; slot ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $nodes.push($ctor($crate::slot_self!($field)));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    (@is $nodes:ident ; $other:ident ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!($ctor ( $field )));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_fragment {
    ($($body:tt)*) => {{
        let mut __cappy_nodes: ::std::vec::Vec<$crate::__private::Element> = ::std::vec::Vec::new();
        $crate::__ui_collect!(__cappy_nodes ; $($body)*);
        $crate::__private::fragment(__cappy_nodes)
    }};
}

/// Element closure for `show` and `if`.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_view {
    ($($closure:tt)*) => {
        $crate::ui_closure!($($closure)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_show_expr {
    (: $cond:expr ; $($body:tt)*) => {
        $crate::__private::show(
            $crate::ui_closure!(move || $cond),
            $crate::__ui_view!(move || $crate::__ui_fragment!($($body)*)),
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_for_expr {
    ($item:ident ; $source:expr ; $key:ident ; self ; $($body:tt)*) => {
        $crate::__private::for_each(
            $crate::ui_closure!(move |visit| {
                $source.with(|list| {
                    for item in list.iter() {
                        visit(item);
                    }
                });
            }),
            |$key| $key,
            $crate::ui_closure!(move |$item| $crate::__ui_fragment!($($body)*)),
        )
    };
    ($item:ident ; $source:expr ; $key:ident ; path $($field:ident).+ ; $($body:tt)*) => {
        $crate::__private::for_each(
            $crate::ui_closure!(move |visit| {
                $source.with(|list| {
                    for item in list.iter() {
                        visit(item);
                    }
                });
            }),
            |$key| &$key$(.$field)+,
            $crate::ui_closure!(move |$item| $crate::__ui_fragment!($($body)*)),
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_if_root {
    ($cond:expr ; $then:expr ; $($rest:tt)*) => {{
        let mut __cappy_nodes: ::std::vec::Vec<$crate::__private::Element> = ::std::vec::Vec::new();
        $crate::__ui_if_chain!(
            @start __cappy_nodes ;
            $cond ;
            $then ;
            $($rest)*
        );
        $crate::__private::fragment(__cappy_nodes)
    }};
}

/// Parses `else` / `else if` and pushes one `if_else` element, then the following siblings.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_if_chain {
    (@start $nodes:ident ; $cond:expr ; $then:expr ; $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @more $nodes ;
            [{$cond ; $then}] ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; [$($frames:tt)*] ; else if ( : $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @more $nodes ;
            [$($frames)* {
                $crate::ui_closure!(move || $cond) ;
                $crate::__ui_view!(move || $crate::__ui_fragment!($($body)*))
            }] ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; [$($frames:tt)*] ; else if ( $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @more $nodes ;
            [$($frames)* {
                $crate::ui_closure!(move || $cond) ;
                $crate::__ui_view!(move || $crate::__ui_fragment!($($body)*))
            }] ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; [$($frames:tt)*] ; else { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            $nodes ;
            [$($frames)*] ;
            $crate::__ui_view!(move || $crate::__ui_fragment!($($body)*)) ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; [$($frames:tt)*] ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            $nodes ;
            [$($frames)*] ;
            || $crate::__private::fragment(::std::vec::Vec::<$crate::__private::Element>::new()) ;
            $($rest)*
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_if_fold {
    ($nodes:ident ; [$($frames:tt)*] ; $else_branch:expr ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @rev $nodes ;
            [] [$($frames)*] ;
            $else_branch ;
            $($rest)*
        )
    };
    (@rev $nodes:ident ; [$($rev:tt)*] [] ; $else_branch:expr ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @nest $nodes ;
            $else_branch ;
            [$($rev)*] ;
            $($rest)*
        )
    };
    (@rev $nodes:ident ; [$($rev:tt)*] [{$cond:expr ; $then:expr} $($more:tt)*] ; $else_branch:expr ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @rev $nodes ;
            [{$cond ; $then} $($rev)*] [$($more)*] ;
            $else_branch ;
            $($rest)*
        )
    };
    (@nest $nodes:ident ; $else_branch:expr ; [{$cond:expr ; $then:expr}] ; $($rest:tt)*) => {
        $nodes.push($crate::__private::if_else($cond, $then, $else_branch));
        $crate::__ui_collect!($nodes ; $($rest)*)
    };
    (@nest $nodes:ident ; $else_branch:expr ; [{$cond:expr ; $then:expr} $($more:tt)+] ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @wrap $nodes ;
            $crate::__private::if_else($cond, $then, $else_branch) ;
            [$($more)+] ;
            $($rest)*
        )
    };
    (@wrap $nodes:ident ; $inner:expr ; [{$cond:expr ; $then:expr} $($more:tt)*] ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @nest $nodes ;
            $crate::__ui_view!(move || $inner) ;
            [{$cond ; $then} $($more)*] ;
            $($rest)*
        )
    };
}

/// `slot(field)` becomes `slot(self.field)`. The captured `slot` token keeps go-to-definition.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_slot_child {
    ($parent:expr ; $ctor:ident ; $field:ident ; $($rest:tt)*) => {
        $crate::__ui_slot_child!(@is $parent ; $ctor ; $field ; $ctor ; $($rest)*)
    };
    (@is $parent:expr ; slot ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $crate::__ui_children!($parent.child($ctor($crate::slot_self!($field))) ; $($rest)*)
    };
    (@is $parent:expr ; $other:ident ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $crate::__ui_children!(
            $parent.child($crate::__ui_node!($ctor ( $field ))) ; $($rest)*
        )
    };
}

/// Places each child of `@Type` into a slot, or appends it when the component has none.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_slot_children {
    ($parent:expr ; $type:ident $(:: $segment:ident)* ;) => {
        $parent
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; , $($rest:tt)*) => {
        $crate::__ui_slot_children!($parent ; $type $(:: $segment)* ; $($rest)*)
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; { .. $children:expr } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_children($children) ; $type $(:: $segment)* ; $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; { $child:expr } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($child) ; $type $(:: $segment)* ; $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; show ( : $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!(show ( : $cond ) { $($body)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; show ( $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!(show ( $cond ) { $($body)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; for ( $item:ident in : $source:expr , key = $key:ident . $($field:ident).+ ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!(for ( $item in : $source , key = $key . $($field).+ ) { $($body)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; for ( $item:ident in : $source:expr , key = $key:ident ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!(for ( $item in : $source , key = $key ) { $($body)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; if ( : $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $parent.place_child($crate::__ui_node!(if ( : $cond ) { $($then)* } $($rest)*))
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; if ( $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $parent.place_child($crate::__ui_node!(if ( $cond ) { $($then)* } $($rest)*))
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; $ctor:ident ( $field:ident ) { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_slot_expr!($ctor ; $field ; [ $($inner)* ])) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; $ctor:ident ( $field:ident ) $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_slot_expr!($ctor ; $field ; [])) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident $(:: $child_segment:ident)* ( $($args:tt)* ) { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_partition_slot!(
            @start
            parent $parent ;
            ty $type $(:: $segment)* ;
            rest [ $($rest)* ] ;
            build [ $name $(:: $child_segment)* ] ;
            inner { $($inner)* } ;
            $($args)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident $(:: $child_segment:ident)* ( $($args:tt)* ) $($rest:tt)*) => {
        $crate::__ui_partition_slot!(
            @start
            parent $parent ;
            ty $type $(:: $segment)* ;
            rest [ $($rest)* ] ;
            build [ $name $(:: $child_segment)* ] ;
            inner { } ;
            $($args)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident $(:: $child_segment:ident)* { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!($name $(:: $child_segment)* { $($inner)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    ($parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident $(:: $child_segment:ident)* $($rest:tt)*) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!($name $(:: $child_segment)*)) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
}

/// Removes `slot = name` from one child's arguments and places that child.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_partition_slot {
    (
        @start
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        $($args:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [] ;
            slot [] ;
            $($args)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $slot:ident ] ;
        slot = $ignored:ident, $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* ] ;
            slot [ $slot ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ ] ;
        slot = $slot:ident, $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* ] ;
            slot [ $slot ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $slot:ident ] ;
        slot = $ignored:ident
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* ] ;
            slot [ $slot ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ ] ;
        slot = $slot:ident
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* ] ;
            slot [ $slot ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        class = [ $($class:tt)* ], $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* class = [ $($class)* ], ] ;
            slot [ $($slot)? ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        class = [ $($class:tt)* ]
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* class = [ $($class)* ], ] ;
            slot [ $($slot)? ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        : class = [ $($class:tt)* ], $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* : class = [ $($class)* ], ] ;
            slot [ $($slot)? ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        : class = [ $($class:tt)* ]
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* : class = [ $($class)* ], ] ;
            slot [ $($slot)? ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        : $field:ident = $value:expr, $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* : $field = $value, ] ;
            slot [ $($slot)? ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        : $field:ident = $value:expr
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* : $field = $value, ] ;
            slot [ $($slot)? ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        $field:ident = $value:expr, $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* $field = $value, ] ;
            slot [ $($slot)? ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        $field:ident = $value:expr
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* $field = $value, ] ;
            slot [ $($slot)? ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        : $content:expr, $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* : $content, ] ;
            slot [ $($slot)? ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        : $content:expr
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* : $content, ] ;
            slot [ $($slot)? ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        $content:expr, $($more:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* $content, ] ;
            slot [ $($slot)? ] ;
            $($more)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $($slot:ident)? ] ;
        $content:expr
    ) => {
        $crate::__ui_partition_slot!(
            @go
            parent $parent ;
            ty $($ty)::+ ;
            rest [ $($rest)* ] ;
            build [ $($build)+ ] ;
            inner { $($inner)* } ;
            kept [ $($kept)* $content, ] ;
            slot [ $($slot)? ] ;
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $slot:ident ] ;
    ) => {
        $crate::__ui_slot_children!(
            $parent.fill_slot(
                <$crate::__ui_type_path!($($ty)::+)>::$slot,
                $crate::__ui_node!($($build)+ ( $($kept)* ) { $($inner)* })
            ) ;
            $($ty)::+ ;
            $($rest)*
        )
    };
    (
        @go
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ ] ;
    ) => {
        $crate::__ui_slot_children!(
            $parent.place_child($crate::__ui_node!($($build)+ ( $($kept)* ) { $($inner)* })) ;
            $($ty)::+ ;
            $($rest)*
        )
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
