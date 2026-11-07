//! Derive macros for cappy.
//!
//! - `#[derive(Props)]` generates builder setters for a component's fields.
//! - `#[derive(ClassName)]` turns a fieldless enum into typo-proof theme class names.
//! - `#[derive(Reactive)]` stores a plain `'static` struct or enum in a [`Reactive`](cappy::Reactive)
//!   handle. [`Reactive`](cappy::Reactive) itself, a [`Signal`](cappy::Signal), and a `#[store]` or
//!   `#[component]` struct stay as they are, so this cannot be a blanket impl.
//! - `stylesheet!` defines those class names together with their style rules.
//! - `#[store]` rewrites a struct in place so each field is tracked and the struct keeps its name.
//!   `cx.use_store(Type)` returns the one value of that struct in the context.
//! - `#[component]` is a reactive struct whose plain fields are state, `#[prop]` fields are inputs,
//!   `#[event]` fields are listeners and `#[slot]` fields are positions for parent children.
//!   `#[props]`, `#[events]`, `#[slots]` and `#[state]` start a section that runs through the
//!   following fields. `#[props]` here is not `#[props(skip)]` from `#[derive(Props)]`.

mod class_name;
mod component;
mod derive_reactive;
mod props;
mod reactive;
mod stylesheet;
mod ui_closure;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, parse_macro_input};

/// Generates a chainable setter per named field: `fn field(self, value: impl Into<T>) -> Self`.
/// For `Option<T>` fields the setter takes `impl Into<T>` and stores `Some`.
/// Mark a field `#[props(skip)]` to leave it without a setter.
///
/// ```
/// use cappy_macros::Props;
///
/// #[derive(Default, Props)]
/// struct Button {
///     label: String,
///     icon: Option<String>,
///     #[props(skip)]
///     clicks: u32,
/// }
///
/// let button = Button::default().label("Save").icon("disk");
/// assert_eq!(button.label, "Save");
/// assert_eq!(button.icon.as_deref(), Some("disk"));
/// # let _ = button.clicks;
/// ```
#[proc_macro_derive(Props, attributes(props))]
pub fn derive_props(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    props::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Stores a plain `'static` struct or enum in a [`Reactive`](cappy::Reactive) handle.
///
/// The generated [`IntoReactive`](cappy::IntoReactive) impl is the one a hand-written plain type
/// would use: [`Reactive::new`](cappy::Reactive::new). Primitives, [`String`](String) and
/// [`Option`](Option) stay special because their prop default is a placeholder. A blanket impl
/// would also clash with [`Reactive`](cappy::Reactive), [`Signal`](cappy::Signal), and with
/// `#[store]` and `#[component]` structs, which must come back unchanged.
///
/// ```ignore
/// #[derive(Clone, Copy, PartialEq, Eq, Default, Reactive)]
/// enum Tab {
///     #[default]
///     Counter,
///     Paint,
/// }
/// ```
#[proc_macro_derive(Reactive)]
pub fn derive_reactive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_reactive::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Maps each variant of a fieldless enum to a kebab-case class name (`BtnPrimary` becomes
/// `"btn-primary"`), overridable with `#[class(name = "...")]`. Generates `as_str`, `ALL`,
/// `Display` and `AsRef<str>`.
///
/// ```
/// use cappy_macros::ClassName;
///
/// #[derive(Clone, Copy, ClassName)]
/// enum Class {
///     Card,
///     BtnPrimary,
///     #[class(name = "title")]
///     Heading,
/// }
///
/// assert_eq!(Class::BtnPrimary.as_str(), "btn-primary");
/// assert_eq!(Class::Heading.to_string(), "title");
/// assert_eq!(Class::ALL.len(), 3);
/// ```

#[proc_macro_derive(ClassName, attributes(class))]
pub fn derive_class_name(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    class_name::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Rewrites a struct in place. Each field type `T` becomes its tracked form, and the struct keeps
/// its name. `new` passes each argument through `reactive`. A field that is already reactive stays
/// that handle.
///
/// The struct implements [`Singleton`](cappy::Singleton). A marker with the struct's name lets
/// `cx.use_store(Type)` returns the one value stored in that [`Context`](cappy::Context). Each field
/// starts from [`Default`](core::default::Default), from the inner default of a
/// [`Reactive`](cappy::Reactive) handle, or from a nested `#[store]` struct. `new` still builds
/// a separate instance.
/// The struct stays [`Clone`](core::clone::Clone) when every tracked field is.
///
/// A `#[component]` struct is already tracked. Remove `#[store]` from it.
#[proc_macro_attribute]
pub fn store(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[store] does not take arguments",
        )
        .into_compile_error()
        .into();
    }
    let input = parse_macro_input!(item as DeriveInput);
    reactive::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A single-file component. Plain fields become reactive state, `#[prop]` fields are parent
/// inputs, `#[event]` fields are listeners, and `#[slot]` fields are positions for parent children.
/// Each of those markers applies to that field only. `#[props]`, `#[events]`, `#[slots]` and
/// `#[state]` apply to that field and every following field until the next section marker.
/// `#[props]` on a component is that section marker. It is not `#[props(skip)]` from
/// `#[derive(Props)]`. A slot field is stored as a [`Copy`](core::marker::Copy) marker, not
/// the child element, and its type must be `Element`. The macro adds `new(cx, ...)`, `open(cx)` and
/// [`Clone`](core::clone::Clone). Do not also derive `Default` or apply `#[store]`. `render` stays
/// a handwritten `Component` impl.
///
/// `class`, `style` and `key` are reserved for the root element.
#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[component] does not take arguments",
        )
        .into_compile_error()
        .into();
    }
    let input = parse_macro_input!(item as DeriveInput);
    component::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Expands to the caller's `cx`. The anchor's span is the call site, so `cx` resolves there
/// instead of inside `ui!`.
#[doc(hidden)]
#[proc_macro]
pub fn context_in_scope(input: TokenStream) -> TokenStream {
    let anchor = parse_macro_input!(input as Ident);
    let cx = Ident::new("cx", anchor.span());
    quote!(#cx).into()
}

/// Expands to `self.field` in the caller's method, so `slot(field)` inside `ui!` reads that field.
#[doc(hidden)]
#[proc_macro]
pub fn slot_self(input: TokenStream) -> TokenStream {
    let field = parse_macro_input!(input as Ident);
    let receiver = Ident::new("self", field.span());
    quote!(#receiver.#field).into()
}

/// Copies each `self.field` used in a `ui!` closure, then builds that closure.
///
/// The copies sit outside the closure, so `text(:format!("{}", self.label))` does not capture
/// `&self`.
#[doc(hidden)]
#[proc_macro]
pub fn ui_closure(input: TokenStream) -> TokenStream {
    ui_closure::expand(input.into()).into()
}

/// Defines class names and their style rules together.
///
/// The enum variants are the classes. `@include Name` copies the properties of another class in
/// this sheet; a property written after the include overwrites a copied one. `Name:hover` and
/// `Name:active` are extra rules for that class, not extra variants. Each one starts from that
/// class's own properties, and a property in the block overwrites one from the base. The macro generates
/// `as_str`, `ALL`, `sheet` and `apply`. `sheet` and `apply` call `Stylesheet`, `StyleRefinement`
/// and `Theme`, which must be in scope.
///
/// ```ignore
/// stylesheet! {
///     enum Class {
///         Header { direction: FlexDirection::Row }
///         HeaderMuted {
///             @include Header,
///             text_color: ColorToken::TextMuted,
///         }
///     }
///     Header:hover { background: ColorToken::SurfaceHover }
/// }
/// ```
#[proc_macro]
pub fn stylesheet(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as stylesheet::Input);
    stylesheet::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
