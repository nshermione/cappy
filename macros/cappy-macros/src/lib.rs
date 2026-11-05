//! Derive macros for cappy.
//!
//! - `#[derive(Props)]` generates builder setters for a component's fields.
//! - `#[derive(ClassName)]` turns a fieldless enum into typo-proof theme class names.
//! - `#[reactive]` rewrites a struct in place so each field is tracked and the struct keeps its name.

mod class_name;
mod props;
mod reactive;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

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
#[proc_macro_attribute]
pub fn reactive(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[reactive] does not take arguments",
        )
        .into_compile_error()
        .into();
    }
    let input = parse_macro_input!(item as DeriveInput);
    reactive::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
