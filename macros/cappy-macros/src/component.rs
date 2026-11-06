use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Field, Generics, Token, Type, parse_quote, spanned::Spanned};

use crate::reactive::{named_fields, tracked_impls, with_into_reactive};

const RESERVED: &[&str] = &["class", "style", "key"];

enum FieldKind {
    State,
    Prop,
    Event,
}

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    if let Some(attr) = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("reactive"))
    {
        return Err(syn::Error::new(
            attr.path().span(),
            "#[component] already tracks its fields; remove #[reactive]",
        ));
    }
    let fields = named_fields(input, "#[component]")?;
    let kinds = fields
        .iter()
        .copied()
        .map(classify)
        .collect::<syn::Result<Vec<_>>>()?;
    let vis = &input.vis;
    let name = &input.ident;
    let attrs = without_default_derive(&input.attrs)?;
    let generics = with_into_reactive(&input.generics);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let struct_fields = fields
        .iter()
        .zip(&kinds)
        .map(|(field, kind)| struct_field(field, kind));
    let new_args = fields.iter().zip(&kinds).filter_map(|(field, kind)| {
        if matches!(kind, FieldKind::Event) {
            None
        } else {
            let field_name = field.ident.as_ref().expect("named field");
            let ty = &field.ty;
            Some(quote!(#field_name: #ty))
        }
    });
    let new_inits = fields
        .iter()
        .zip(&kinds)
        .map(|(field, kind)| field_init(field, kind, false));
    let setters = fields
        .iter()
        .zip(&kinds)
        .filter_map(|(field, kind)| setter(field, kind));
    let default_impl = default_impl(&generics, name, &fields, &kinds);
    let copy_types: Vec<TokenStream> = fields
        .iter()
        .zip(&kinds)
        .filter_map(|(field, kind)| match kind {
            FieldKind::Event => None,
            FieldKind::State | FieldKind::Prop => {
                let ty = &field.ty;
                Some(quote!(<#ty as ::cappy::IntoReactive>::Output))
            }
        })
        .collect();
    let tracked = tracked_impls(name, &generics, &copy_types);

    Ok(quote! {
        #(#attrs)*
        #vis struct #name #impl_generics #where_clause {
            #(#struct_fields)*
        }

        impl #impl_generics #name #type_generics #where_clause {
            pub fn new(#(#new_args),*) -> Self {
                Self {
                    #(#new_inits,)*
                }
            }

            #(#setters)*
        }

        #default_impl
        #tracked
    })
}

fn classify(field: &Field) -> syn::Result<FieldKind> {
    let name = field.ident.as_ref().expect("named field");
    if RESERVED.iter().any(|reserved| name == reserved) {
        return Err(syn::Error::new(
            name.span(),
            format!("`{name}` is reserved for the root element"),
        ));
    }
    let prop = marker(field, "prop")?;
    let event = marker(field, "event")?;
    match (prop, event) {
        (true, true) => Err(syn::Error::new(
            name.span(),
            "a field cannot be both #[prop] and #[event]",
        )),
        (false, true) => Ok(FieldKind::Event),
        (true, false) => Ok(FieldKind::Prop),
        (false, false) => Ok(FieldKind::State),
    }
}

fn marker(field: &Field, name: &str) -> syn::Result<bool> {
    let mut found = false;
    for attr in &field.attrs {
        if !attr.path().is_ident(name) {
            continue;
        }
        if !matches!(attr.meta, syn::Meta::Path(_)) {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!("#[{name}] does not take arguments"),
            ));
        }
        found = true;
    }
    Ok(found)
}

/// Drops a `Default` derive when it is still on the token stream. rustc usually applies derives
/// after this macro, so a written `#[derive(Default)]` still conflicts with the generated impl.
fn without_default_derive(attrs: &[Attribute]) -> syn::Result<Vec<Attribute>> {
    let mut kept_attrs = Vec::with_capacity(attrs.len());
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            kept_attrs.push(attr.clone());
            continue;
        }
        let paths: Punctuated<syn::Path, Token![,]> =
            attr.parse_args_with(Punctuated::parse_terminated)?;
        let original_len = paths.len();
        let kept: Vec<_> = paths
            .into_iter()
            .filter(|path| !path.is_ident("Default"))
            .collect();
        if kept.len() == original_len {
            kept_attrs.push(attr.clone());
        } else if !kept.is_empty() {
            kept_attrs.push(parse_quote!(#[derive(#(#kept),*)]));
        }
    }
    Ok(kept_attrs)
}

fn kept_attrs(field: &Field) -> Vec<&Attribute> {
    field
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("prop") && !attr.path().is_ident("event"))
        .collect()
}

fn struct_field(field: &Field, kind: &FieldKind) -> TokenStream {
    let vis = &field.vis;
    let name = field.ident.as_ref().expect("named field");
    let ty = &field.ty;
    let attrs = kept_attrs(field);
    match kind {
        FieldKind::Event => quote! {
            #(#attrs)*
            #vis #name: ::cappy::Listener<#ty>,
        },
        FieldKind::State | FieldKind::Prop => quote! {
            #(#attrs)*
            #vis #name: <#ty as ::cappy::IntoReactive>::Output,
        },
    }
}

fn field_init(field: &Field, kind: &FieldKind, default_value: bool) -> TokenStream {
    let name = field.ident.as_ref().expect("named field");
    let ty = &field.ty;
    match kind {
        FieldKind::Event => quote!(#name: ::cappy::Listener::empty()),
        FieldKind::State | FieldKind::Prop if default_value => {
            quote!(#name: ::cappy::reactive(<#ty as ::core::default::Default>::default()))
        }
        FieldKind::State | FieldKind::Prop => quote!(#name: ::cappy::reactive(#name)),
    }
}

fn setter(field: &Field, kind: &FieldKind) -> Option<TokenStream> {
    let name = field.ident.as_ref().expect("named field");
    let ty = &field.ty;
    let docs = field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"));
    match kind {
        FieldKind::State => None,
        FieldKind::Prop => Some(quote! {
            #(#docs)*
            #[must_use]
            pub fn #name<__Value, __Marker>(mut self, value: __Value) -> Self
            where
                __Value: ::cappy::PropInput<#ty, __Marker>,
            {
                ::cappy::PropInput::apply(value, &mut self.#name);
                self
            }
        }),
        FieldKind::Event if is_unit(ty) => Some(quote! {
            #(#docs)*
            #[must_use]
            pub fn #name<__Handler>(self, mut handler: __Handler) -> Self
            where
                __Handler: ::core::ops::FnMut() + 'static,
            {
                self.#name.set(move |_: &()| handler());
                self
            }
        }),
        FieldKind::Event => Some(quote! {
            #(#docs)*
            #[must_use]
            pub fn #name<__Handler>(self, handler: __Handler) -> Self
            where
                __Handler: ::core::ops::FnMut(&#ty) + 'static,
            {
                self.#name.set(handler);
                self
            }
        }),
    }
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn default_impl(
    generics: &Generics,
    name: &syn::Ident,
    fields: &[&Field],
    kinds: &[FieldKind],
) -> TokenStream {
    let mut generics = generics.clone();
    {
        let predicates = &mut generics.make_where_clause().predicates;
        for (field, kind) in fields.iter().zip(kinds) {
            if !matches!(kind, FieldKind::Event) {
                let ty = &field.ty;
                predicates.push(parse_quote!(#ty: ::core::default::Default));
            }
        }
    }
    let inits = fields
        .iter()
        .zip(kinds)
        .map(|(field, kind)| field_init(field, kind, true));
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics ::core::default::Default for #name #type_generics #where_clause {
            fn default() -> Self {
                Self {
                    #(#inits,)*
                }
            }
        }
    }
}
