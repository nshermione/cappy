//! Declarative macros for cappy: [`element!`] builds element trees, [`style!`] builds style
//! refinements and [`theme!`] derives themes, all expanding to plain builder calls.

#[doc(hidden)]
pub mod __private {
    pub use cappy_core::*;
}

#[doc(hidden)]
pub use cappy_macros::{slot_self, ui_closure, ui_for_view, ui_view};

/// Builds an element tree.
///
/// ```
/// use cappy_dsl::{style, element};
/// use cappy_core::{Application, Space, div, text};
///
/// let application = Application::new();
/// let cx = application.context();
/// let count = cx.reactive(0_i32);
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
/// - `name(content, field = value, ...) { children }` calls the function `name` in scope, so
///   `div`, `text` and your own element functions all resolve (and go to definition) the same
///   way. The leading `content` argument, the fields and the children are all optional.
/// - `@Type(field = value, ...) { children }` builds `Type::open(&context)` and sets fields.
///   `context` is the name passed to [`element!`](element): `element!((context) { @Type })`. The type is a
///   `#[component]`. `class`, `style` and `key` are applied to the element
///   `render` returns. Without `#[slot]` fields, children are appended to that root. With slots,
///   the first child that has no `slot = name` fills the first slot, `slot = name` fills that
///   slot, and further unnamed children are dropped.
/// - `slot(field)` inside a component's `render` places that `#[slot]` field. It reads
///   `self.field`.
/// - `{ expression }` embeds an element; `{ ..iterator }` embeds every element it yields.
/// - `show (:cond) { ... }` keeps its nodes and hides them while `cond` is false.
/// - `if (:cond) { ... } else { ... }` disposes the branch that is not selected, including
///   nested `show`, `if` and `for`. `else` may be omitted or written `else if`.
/// - `for (item in :list, key = item.field) { ... }` repeats the block, keeping a row when its
///   key stays. `item` in the block is the row signal.
/// - `show`, `if` and `for` build their elements in a `'static` closure. Pass the context when
///   those blocks or `@Type` use it: `element!((context) { ... })`. Each block clones that binding, so
///   several blocks can share it, and the same name is bound again inside the closure. A tree
///   that never opens a component and never reads the context inside those blocks stays
///   `element! { ... }`.
/// - `slot(field)` stays outside `show`, `if` and `for`. A parent fills slots before mount, and
///   those blocks build their contents at mount.
///
/// Every `field = value` calls the builder method `field(value)`. A leading `:` makes the value
/// reactive: `:field = value` calls `field(Dynamic(move || value))` and the content `:value`
/// calls `name(Dynamic(move || value))`, so the value is recomputed whenever a signal it reads
/// changes. `self.field` inside that expression, and inside `show`, `if` and `for`, is copied
/// out before the closure. The setter must accept [`Dynamic`](cappy_core::Dynamic), as `text`, `class`
/// and `style` do.
///
/// `class` takes one class or a list: `class = [a, b]` is shorthand for `class = a, class = b`,
/// and `:class = [a, b]` for `:class = a, :class = b`. List items may mix strings and typed
/// classes.
///
/// Children may be separated by commas. Put a comma before a `{ expression }` child that follows
/// an element with arguments, or it is read as that element's children.
#[macro_export]
macro_rules! element {
    (( $cx:ident ) { $($body:tt)* }) => {
        $crate::__ui_node!(@cx [ $cx ] ; $($body)*)
    };
    ($($body:tt)+) => {
        $crate::__ui_node!(@cx [ ] ; $($body)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_node {
    (@cx [ $($cx:ident)? ] ; show ( : $cond:expr ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_show_expr!(@cx [ $($cx)? ] ; : $cond ; $($body)*)
    };
    (@cx [ $($cx:ident)? ] ; show ( $cond:expr ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_show_expr!(@cx [ $($cx)? ] ; : $cond ; $($body)*)
    };
    (@cx [ $($cx:ident)? ] ; if ( : $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_root!(
            @cx [ $($cx)? ] ;
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($then)*)) ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; if ( $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_root!(
            @cx [ $($cx)? ] ;
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($then)*)) ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; for ( $item:ident in : $source:expr , key = $key:ident . $($field:ident).+ ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_for_expr!(@cx [ $($cx)? ] ; $item ; $source ; $key ; path $($field).+ ; $($body)*)
    };
    (@cx [ $($cx:ident)? ] ; for ( $item:ident in : $source:expr , key = $key:ident ) { $($body:tt)* } $(,)?) => {
        $crate::__ui_for_expr!(@cx [ $($cx)? ] ; $item ; $source ; $key ; self ; $($body)*)
    };
    (@cx [ $($cx:ident)? ] ; @ $type:ident $(:: $segment:ident)* $(( $($fields:tt)* ))? $({ $($children:tt)* })? $(,)?) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [] ;
            host [] ;
            children [ $($($children)*)? ] ;
            $($($fields)*)?
        )
    };
    (@cx [ $($cx:ident)? ] ; { $element:expr } $(,)?) => {
        $crate::__private::Element::from($element)
    };
    (@cx [ $($cx:ident)? ] ; $ctor:ident ( $field:ident ) $(,)?) => {
        $crate::__ui_slot_expr!($ctor ; $field)
    };
    (@cx [ $($cx:ident)? ] ; $name:ident $(( $($args:tt)* ))? $({ $($children:tt)* })? $(,)?) => {
        $crate::__ui_children!(
            @cx [ $($cx)? ] ;
            $crate::__ui_args!(@head $name ; $($($args)*)?) ;
            $($($children)*)?
        )
    };
}

/// Splits `@Type` arguments. `class`, `style` and `key` stay on the element `render` returns.
/// Every other field is a setter on the component. Children fill slots, or append to that root
/// when the component has none.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_component {
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        class = [ $($class:expr),* $(,)? ] $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* class = [ $($class),* ], ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : class = [ $($class:expr),* $(,)? ] $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : class = [ $($class),* ], ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        class = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* class = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : class = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : class = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        style = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* style = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : style = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : style = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        key = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* key = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : key = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* ] ;
            host [ $($host)* : key = $value, ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        : $field:ident = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* : $field = $value, ] ;
            host [ $($host)* ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $($cx:ident)? ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
        $field:ident = $value:expr $(, $($rest:tt)*)?
    ) => {
        $crate::__ui_component!(
            @cx [ $($cx)? ] ;
            $type $(:: $segment)* ;
            props [ $($props)* $field = $value, ] ;
            host [ $($host)* ] ;
            children [ $($children)* ] ;
            $($($rest)*)?
        )
    };
    (
        @cx [ $cx:ident ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
    ) => {
        $crate::__ui_args!(
            @fields
            [
                $crate::__ui_slot_children!(
                    @cx [ $cx ] ;
                    $crate::__private::Element::from(
                        $crate::__ui_args!(@fields [<$type $(:: $segment)*>::open(&$cx)] $($props)*)
                    ).prepare_slots() ;
                    $type $(:: $segment)* ;
                    $($children)*
                )
            ]
            $($host)*
        )
    };
    (
        @cx [ ] ;
        $type:ident $(:: $segment:ident)* ;
        props [ $($props:tt)* ] ;
        host [ $($host:tt)* ] ;
        children [ $($children:tt)* ] ;
    ) => {
        ::core::compile_error!(
            "pass the context to element! before using @Type: element!((cx) { @Type(...) })"
        )
    };
}

/// `slot(field)` reads `self.field`. Any other single-ident call stays a normal element.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_slot_expr {
    ($ctor:ident ; $field:ident) => {
        $crate::__ui_slot_expr!(@is $ctor ; $field ; $ctor)
    };
    (@is slot ; $field:ident ; $ctor:ident) => {
        $ctor($crate::slot_self!($field))
    };
    (@is $other:ident ; $field:ident ; $ctor:ident) => {
        $crate::__ui_args!(@head $ctor ; $field)
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
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $($body:tt)*) => {{
        let mut __cappy_nodes: ::std::vec::Vec<$crate::__private::Element> = ::std::vec::Vec::new();
        $crate::__ui_collect!(@cx [ $($cx)? ] ; __cappy_nodes ; $($body)*);
        __cappy_nodes
            .into_iter()
            .fold($parent, |parent, child| parent.child(child))
    }};
}

/// Pushes each child element into `$nodes`.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_collect {
    (@cx [ $($cx:ident)? ] ; $nodes:ident ;) => {};
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; , $($rest:tt)*) => {
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; { .. $children:expr } $($rest:tt)*) => {
        $nodes.extend($children.into_iter().map($crate::__private::Element::from));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; { $child:expr } $($rest:tt)*) => {
        $nodes.push($crate::__private::Element::from($child));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; show ( : $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_show_expr!(@cx [ $($cx)? ] ; : $cond ; $($body)*));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; show ( $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_show_expr!(@cx [ $($cx)? ] ; : $cond ; $($body)*));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; if ( : $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @start $nodes ;
            @cx [ $($cx)? ] ;
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($then)*)) ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; if ( $cond:expr ) { $($then:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @start $nodes ;
            @cx [ $($cx)? ] ;
            $crate::ui_closure!(move || $cond) ;
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($then)*)) ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; for ( $item:ident in : $source:expr , key = $key:ident . $($field:ident).+ ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_for_expr!(@cx [ $($cx)? ] ; $item ; $source ; $key ; path $($field).+ ; $($body)*));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; for ( $item:ident in : $source:expr , key = $key:ident ) { $($body:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_for_expr!(@cx [ $($cx)? ] ; $item ; $source ; $key ; self ; $($body)*));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $ctor:ident ( $field:ident ) { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; $ctor ( $field ) { $($children)* }));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $ctor:ident ( $field:ident ) $($rest:tt)*) => {
        $crate::__ui_collect_slot!(@cx [ $($cx)? ] ; $nodes ; $ctor ; $field ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; @ $type:ident $(:: $segment:ident)* ( $($props:tt)* ) { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; @ $type $(:: $segment)* ( $($props)* ) { $($children)* }));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; @ $type:ident $(:: $segment:ident)* ( $($props:tt)* ) $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; @ $type $(:: $segment)* ( $($props)* )));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; @ $type:ident $(:: $segment:ident)* { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; @ $type $(:: $segment)* { $($children)* }));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; @ $type:ident $(:: $segment:ident)* $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; @ $type $(:: $segment)*));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $name:ident ( $($args:tt)* ) { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; $name ( $($args)* ) { $($children)* }));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $name:ident ( $($args:tt)* ) $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; $name ( $($args)* )));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $name:ident { $($children:tt)* } $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; $name { $($children)* }));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $name:ident $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; $name));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_collect_slot {
    (@cx [ $($cx:ident)? ] ; $nodes:ident ; $ctor:ident ; $field:ident ; $($rest:tt)*) => {
        $crate::__ui_collect_slot!(@is @cx [ $($cx)? ] ; $nodes ; $ctor ; $field ; $ctor ; $($rest)*)
    };
    (@is @cx [ $($cx:ident)? ] ; $nodes:ident ; slot ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $nodes.push($ctor($crate::slot_self!($field)));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@is @cx [ $($cx:ident)? ] ; $nodes:ident ; $other:ident ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $nodes.push($crate::__ui_node!(@cx [ $($cx)? ] ; $ctor ( $field )));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_fragment {
    (@cx [ $($cx:ident)? ] ; $($body:tt)*) => {{
        let mut __cappy_nodes: ::std::vec::Vec<$crate::__private::Element> = ::std::vec::Vec::new();
        $crate::__ui_collect!(@cx [ $($cx)? ] ; __cappy_nodes ; $($body)*);
        $crate::__private::fragment(__cappy_nodes)
    }};
}

/// Element closure for `show` and `if`. With a context name, clones that binding into the closure.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_view {
    (@cx [ $cx:ident ] ; $($closure:tt)*) => {
        $crate::ui_view!(
            $crate::__private::with_context ;
            $cx ;
            $($closure)*
        )
    };
    (@cx [ ] ; $($closure:tt)*) => {
        $crate::ui_closure!($($closure)*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_show_expr {
    (@cx [ $($cx:ident)? ] ; : $cond:expr ; $($body:tt)*) => {
        $crate::__private::show(
            $crate::ui_closure!(move || $cond),
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($body)*)),
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_for_expr {
    (@cx [ $cx:ident ] ; $item:ident ; $source:expr ; $key:ident ; self ; $($body:tt)*) => {
        $crate::ui_for_view!(
            $crate::__private::for_each ;
            $crate::__private::Context ;
            $cx ;
            $crate::ui_closure!(move |visit| {
                $source.with(|list| {
                    for item in list.iter() {
                        visit(item);
                    }
                });
            }) ;
            |$key| $key ;
            move |$item| $crate::__ui_fragment!(@cx [ $cx ] ; $($body)*)
        )
    };
    (@cx [ ] ; $item:ident ; $source:expr ; $key:ident ; self ; $($body:tt)*) => {
        $crate::__private::for_each(
            $crate::ui_closure!(move |visit| {
                $source.with(|list| {
                    for item in list.iter() {
                        visit(item);
                    }
                });
            }),
            |$key| $key,
            $crate::ui_closure!(move |$item| $crate::__ui_fragment!(@cx [ ] ; $($body)*)),
        )
    };
    (@cx [ $cx:ident ] ; $item:ident ; $source:expr ; $key:ident ; path $($field:ident).+ ; $($body:tt)*) => {
        $crate::ui_for_view!(
            $crate::__private::for_each ;
            $crate::__private::Context ;
            $cx ;
            $crate::ui_closure!(move |visit| {
                $source.with(|list| {
                    for item in list.iter() {
                        visit(item);
                    }
                });
            }) ;
            |$key| &$key$(.$field)+ ;
            move |$item| $crate::__ui_fragment!(@cx [ $cx ] ; $($body)*)
        )
    };
    (@cx [ ] ; $item:ident ; $source:expr ; $key:ident ; path $($field:ident).+ ; $($body:tt)*) => {
        $crate::__private::for_each(
            $crate::ui_closure!(move |visit| {
                $source.with(|list| {
                    for item in list.iter() {
                        visit(item);
                    }
                });
            }),
            |$key| &$key$(.$field)+,
            $crate::ui_closure!(move |$item| $crate::__ui_fragment!(@cx [ ] ; $($body)*)),
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_if_root {
    (@cx [ $($cx:ident)? ] ; $cond:expr ; $then:expr ; $($rest:tt)*) => {{
        let mut __cappy_nodes: ::std::vec::Vec<$crate::__private::Element> = ::std::vec::Vec::new();
        $crate::__ui_if_chain!(
            @start __cappy_nodes ;
            @cx [ $($cx)? ] ;
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
    (@start $nodes:ident ; @cx [ $($cx:ident)? ] ; $cond:expr ; $then:expr ; $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @more $nodes ;
            @cx [ $($cx)? ] ;
            [{$cond ; $then}] ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; @cx [ $($cx:ident)? ] ; [$($frames:tt)*] ; else if ( : $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @more $nodes ;
            @cx [ $($cx)? ] ;
            [$($frames)* {
                $crate::ui_closure!(move || $cond) ;
                $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($body)*))
            }] ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; @cx [ $($cx:ident)? ] ; [$($frames:tt)*] ; else if ( $cond:expr ) { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_chain!(
            @more $nodes ;
            @cx [ $($cx)? ] ;
            [$($frames)* {
                $crate::ui_closure!(move || $cond) ;
                $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($body)*))
            }] ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; @cx [ $($cx:ident)? ] ; [$($frames:tt)*] ; else { $($body:tt)* } $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            $nodes ;
            @cx [ $($cx)? ] ;
            [$($frames)*] ;
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $crate::__ui_fragment!(@cx [ $($cx)? ] ; $($body)*)) ;
            $($rest)*
        )
    };
    (@more $nodes:ident ; @cx [ $($cx:ident)? ] ; [$($frames:tt)*] ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            $nodes ;
            @cx [ $($cx)? ] ;
            [$($frames)*] ;
            || $crate::__private::fragment(::std::vec::Vec::<$crate::__private::Element>::new()) ;
            $($rest)*
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __ui_if_fold {
    ($nodes:ident ; @cx [ $($cx:ident)? ] ; [$($frames:tt)*] ; $else_branch:expr ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @rev $nodes ;
            @cx [ $($cx)? ] ;
            [] [$($frames)*] ;
            $else_branch ;
            $($rest)*
        )
    };
    (@rev $nodes:ident ; @cx [ $($cx:ident)? ] ; [$($rev:tt)*] [] ; $else_branch:expr ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @nest $nodes ;
            @cx [ $($cx)? ] ;
            $else_branch ;
            [$($rev)*] ;
            $($rest)*
        )
    };
    (@rev $nodes:ident ; @cx [ $($cx:ident)? ] ; [$($rev:tt)*] [{$cond:expr ; $then:expr} $($more:tt)*] ; $else_branch:expr ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @rev $nodes ;
            @cx [ $($cx)? ] ;
            [{$cond ; $then} $($rev)*] [$($more)*] ;
            $else_branch ;
            $($rest)*
        )
    };
    (@nest $nodes:ident ; @cx [ $($cx:ident)? ] ; $else_branch:expr ; [{$cond:expr ; $then:expr}] ; $($rest:tt)*) => {
        $nodes.push($crate::__private::if_else($cond, $then, $else_branch));
        $crate::__ui_collect!(@cx [ $($cx)? ] ; $nodes ; $($rest)*)
    };
    (@nest $nodes:ident ; @cx [ $($cx:ident)? ] ; $else_branch:expr ; [{$cond:expr ; $then:expr} $($more:tt)+] ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @wrap $nodes ;
            @cx [ $($cx)? ] ;
            $crate::__private::if_else($cond, $then, $else_branch) ;
            [$($more)+] ;
            $($rest)*
        )
    };
    (@wrap $nodes:ident ; @cx [ $($cx:ident)? ] ; $inner:expr ; [{$cond:expr ; $then:expr} $($more:tt)*] ; $($rest:tt)*) => {
        $crate::__ui_if_fold!(
            @nest $nodes ;
            @cx [ $($cx)? ] ;
            $crate::__ui_view!(@cx [ $($cx)? ] ; move || $inner) ;
            [{$cond ; $then} $($more)*] ;
            $($rest)*
        )
    };
}

/// `slot(field)` becomes `slot(self.field)`. The captured `slot` token keeps go-to-definition.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_slot_child {
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $ctor:ident ; $field:ident ; $($rest:tt)*) => {
        $crate::__ui_slot_child!(@is @cx [ $($cx)? ] ; $parent ; $ctor ; $field ; $ctor ; $($rest)*)
    };
    (@is @cx [ $($cx:ident)? ] ; $parent:expr ; slot ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $crate::__ui_children!(@cx [ $($cx)? ] ; $parent.child($ctor($crate::slot_self!($field))) ; $($rest)*)
    };
    (@is @cx [ $($cx:ident)? ] ; $parent:expr ; $other:ident ; $field:ident ; $ctor:ident ; $($rest:tt)*) => {
        $crate::__ui_children!(@cx [ $($cx)? ] ;
            $parent.child($crate::__ui_node!(@cx [ $($cx)? ] ; $ctor ( $field ))) ; $($rest)*
        )
    };
}

/// Places each child of `@Type` into a slot, or appends it when the component has none.
#[doc(hidden)]
#[macro_export]
macro_rules! __ui_slot_children {
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ;) => {
        $parent
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; , $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ; $parent ; $type $(:: $segment)* ; $($rest)*)
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; { .. $children:expr } $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_children($children) ; $type $(:: $segment)* ; $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; { $child:expr } $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_child($child) ; $type $(:: $segment)* ; $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; @ $child:ident $(:: $child_segment:ident)* ( $($props:tt)* ) { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_partition_slot!(
            @start
            cx [ $($cx)? ] ;
            parent $parent ;
            ty $type $(:: $segment)* ;
            rest [ $($rest)* ] ;
            build [ @ $child $(:: $child_segment)* ] ;
            inner { $($inner)* } ;
            $($props)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; @ $child:ident $(:: $child_segment:ident)* ( $($props:tt)* ) $($rest:tt)*) => {
        $crate::__ui_partition_slot!(
            @start
            cx [ $($cx)? ] ;
            parent $parent ;
            ty $type $(:: $segment)* ;
            rest [ $($rest)* ] ;
            build [ @ $child $(:: $child_segment)* ] ;
            inner { } ;
            $($props)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; @ $child:ident $(:: $child_segment:ident)* { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_child($crate::__ui_node!(@cx [ $($cx)? ] ; @ $child $(:: $child_segment)* { $($inner)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; @ $child:ident $(:: $child_segment:ident)* $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_child($crate::__ui_node!(@cx [ $($cx)? ] ; @ $child $(:: $child_segment)*)) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident ( $($args:tt)* ) { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_partition_slot!(
            @start
            cx [ $($cx)? ] ;
            parent $parent ;
            ty $type $(:: $segment)* ;
            rest [ $($rest)* ] ;
            build [ $name ] ;
            inner { $($inner)* } ;
            $($args)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident ( $($args:tt)* ) $($rest:tt)*) => {
        $crate::__ui_partition_slot!(
            @start
            cx [ $($cx)? ] ;
            parent $parent ;
            ty $type $(:: $segment)* ;
            rest [ $($rest)* ] ;
            build [ $name ] ;
            inner { } ;
            $($args)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident { $($inner:tt)* } $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_child($crate::__ui_node!(@cx [ $($cx)? ] ; $name { $($inner)* })) ;
            $type $(:: $segment)* ;
            $($rest)*
        )
    };
    (@cx [ $($cx:ident)? ] ; $parent:expr ; $type:ident $(:: $segment:ident)* ; $name:ident $($rest:tt)*) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_child($crate::__ui_node!(@cx [ $($cx)? ] ; $name)) ;
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
        cx [ $($cx:ident)? ] ;
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        $($args:tt)*
    ) => {
        $crate::__ui_partition_slot!(
            @go
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
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
            cx [ $($cx)? ] ;
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
        cx [ $($cx:ident)? ] ;
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ $slot:ident ] ;
    ) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.fill_slot(
                <$($ty)::*>::$slot,
                $crate::__ui_node!(@cx [ $($cx)? ] ; $($build)+ ( $($kept)* ) { $($inner)* })
            ) ;
            $($ty)::+ ;
            $($rest)*
        )
    };
    (
        @go
        cx [ $($cx:ident)? ] ;
        parent $parent:expr ;
        ty $($ty:ident)::+ ;
        rest [ $($rest:tt)* ] ;
        build [ $($build:tt)+ ] ;
        inner { $($inner:tt)* } ;
        kept [ $($kept:tt)* ] ;
        slot [ ] ;
    ) => {
        $crate::__ui_slot_children!(@cx [ $($cx)? ] ;
            $parent.place_child($crate::__ui_node!(@cx [ $($cx)? ] ; $($build)+ ( $($kept)* ) { $($inner)* })) ;
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
