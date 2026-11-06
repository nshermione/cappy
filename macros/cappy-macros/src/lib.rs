//! Derive macros for cappy.
//!
//! - `#[derive(Props)]` generates builder setters for a component's fields.
//! - `#[derive(ClassName)]` turns a fieldless enum into typo-proof theme class names.
//! - `stylesheet!` defines those class names together with their style rules.
//! - `#[reactive]` rewrites a struct in place so each field is tracked and the struct keeps its name.
//!   `#[reactive(singleton)]` also adds one thread-local `singleton()`.
//! - `#[component]` is a reactive struct whose plain fields are state, `#[prop]` fields are inputs
//!   and `#[event]` fields are listeners.

mod class_name;
mod component;
mod props;
mod reactive;
mod stylesheet;

use proc_macro::TokenStream;
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
/// `#[reactive(singleton)]` also adds `singleton()`: one value per thread, stored inside that
/// function, starting from [`Default`](core::default::Default) for each field. The struct must
/// stay [`Copy`](core::marker::Copy), which it does when every tracked field is.
///
/// A `#[component]` struct is already tracked. Remove `#[reactive]` from it.
#[proc_macro_attribute]
pub fn reactive(attr: TokenStream, item: TokenStream) -> TokenStream {
    let singleton = match singleton_attribute(attr) {
        Ok(singleton) => singleton,
        Err(error) => return error.into_compile_error().into(),
    };
    let input = parse_macro_input!(item as DeriveInput);
    reactive::expand(&input, singleton)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[reactive]` or `#[reactive(singleton)]`. Anything else is a compile error.
fn singleton_attribute(attr: TokenStream) -> syn::Result<bool> {
    if attr.is_empty() {
        return Ok(false);
    }
    let ident = syn::parse::<Ident>(attr)?;
    if ident == "singleton" {
        return Ok(true);
    }
    Err(syn::Error::new(
        ident.span(),
        "unknown #[reactive] option; expected `singleton`",
    ))
}

/// A single-file component. Plain fields become reactive state, `#[prop]` fields are parent
/// inputs, and `#[event]` fields are listeners. The macro implements [`Default`](core::default::Default).
/// Do not also derive `Default` or apply `#[reactive]`. `render` stays a handwritten `Component` impl.
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
