use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Field, Generics, Token, Type, parse_quote, spanned::Spanned};

use crate::reactive::{named_fields, tracked_impls, with_into_reactive};

const RESERVED: &[&str] = &["class", "style", "key", "context"];

#[derive(Clone, Copy)]
enum Section {
    State,
    Prop,
    Event,
    Slot,
}

struct KindAttr {
    name: &'static str,
    section: Section,
    /// A section marker also applies to every following field until the next one.
    sticks: bool,
}

const KIND_ATTRS: &[KindAttr] = &[
    KindAttr {
        name: "prop",
        section: Section::Prop,
        sticks: false,
    },
    KindAttr {
        name: "props",
        section: Section::Prop,
        sticks: true,
    },
    KindAttr {
        name: "event",
        section: Section::Event,
        sticks: false,
    },
    KindAttr {
        name: "events",
        section: Section::Event,
        sticks: true,
    },
    KindAttr {
        name: "slot",
        section: Section::Slot,
        sticks: false,
    },
    KindAttr {
        name: "slots",
        section: Section::Slot,
        sticks: true,
    },
    KindAttr {
        name: "state",
        section: Section::State,
        sticks: true,
    },
];

enum FieldKind {
    State,
    Prop,
    Event,
    Slot(u16),
}

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    if let Some(attr) = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("store"))
    {
        return Err(syn::Error::new(
            attr.path().span(),
            "#[component] already tracks its fields; remove #[store]",
        ));
    }
    let fields = named_fields(input, "#[component]")?;
    let mut kinds = Vec::with_capacity(fields.len());
    let mut section = Section::State;
    let mut next_slot = 0u16;
    for field in &fields {
        kinds.push(classify(field, &mut section, &mut next_slot)?);
    }
    let vis = &input.vis;
    let name = &input.ident;
    let attrs = without_default_derive(&input.attrs)?;
    let mut generics = with_into_reactive(&input.generics);
    let struct_fields = fields
        .iter()
        .zip(&kinds)
        .map(|(field, kind)| struct_field(field, kind));
    let new_args = fields.iter().zip(&kinds).filter_map(|(field, kind)| {
        if matches!(kind, FieldKind::Event | FieldKind::Slot(_)) {
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
    let slot_consts = fields.iter().zip(&kinds).filter_map(|(field, kind)| {
        let FieldKind::Slot(index) = kind else {
            return None;
        };
        let name = field.ident.as_ref().expect("named field");
        let ty = &field.ty;
        Some(quote! {
            #[allow(non_upper_case_globals, dead_code)]
            pub const #name: ::cappy::Slot<Self, #ty> = ::cappy::Slot::new(#index);
        })
    });
    let copy_types: Vec<TokenStream> = fields
        .iter()
        .zip(&kinds)
        .filter_map(|(field, kind)| match kind {
            FieldKind::Event | FieldKind::Slot(_) => None,
            FieldKind::State | FieldKind::Prop => {
                let ty = &field.ty;
                Some(quote!(<#ty as ::cappy::IntoReactive>::Output))
            }
        })
        .collect();
    {
        let predicates = &mut generics.make_where_clause().predicates;
        for ty in &copy_types {
            predicates.push(parse_quote!(#ty: ::core::clone::Clone));
        }
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let tracked = tracked_impls(name, &generics, &copy_types);

    let open = open_method(&generics, name, &fields, &kinds);
    Ok(quote! {
        #[derive(Clone)]
        #(#attrs)*
        #vis struct #name #impl_generics #where_clause {
            context: ::cappy::Context,
            #(#struct_fields)*
        }

        impl #impl_generics #name #type_generics #where_clause {
            pub fn new(cx: &::cappy::Context, #(#new_args),*) -> Self {
                Self {
                    context: cx.clone(),
                    #(#new_inits,)*
                }
            }

            #open
            #(#setters)*
            #(#slot_consts)*
        }

        impl #impl_generics ::cappy::HasContext for #name #type_generics #where_clause {
            fn context(&self) -> &::cappy::Context {
                &self.context
            }
        }

        #tracked
    })
}

fn classify(field: &Field, section: &mut Section, next_slot: &mut u16) -> syn::Result<FieldKind> {
    let name = field.ident.as_ref().expect("named field");
    if RESERVED.iter().any(|reserved| name == reserved) {
        return Err(syn::Error::new(
            name.span(),
            format!("`{name}` is reserved for the root element"),
        ));
    }
    let chosen = match field_marker(field)? {
        Some(marker) if marker.sticks => {
            *section = marker.section;
            marker.section
        }
        Some(marker) => marker.section,
        None => *section,
    };
    match chosen {
        Section::State => Ok(FieldKind::State),
        Section::Prop => Ok(FieldKind::Prop),
        Section::Event => Ok(FieldKind::Event),
        Section::Slot => {
            if !is_element_type(&field.ty) {
                return Err(syn::Error::new(
                    field.ty.span(),
                    "#[slot] fields must have type `Element`",
                ));
            }
            let index = *next_slot;
            *next_slot = next_slot
                .checked_add(1)
                .ok_or_else(|| syn::Error::new(name.span(), "too many #[slot] fields"))?;
            Ok(FieldKind::Slot(index))
        }
    }
}

fn is_element_type(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() {
        return false;
    }
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    segment.ident == "Element" && segment.arguments.is_empty()
}

fn field_marker(field: &Field) -> syn::Result<Option<&KindAttr>> {
    let mut found: Option<&KindAttr> = None;
    for attr in &field.attrs {
        let Some(kind) = KIND_ATTRS
            .iter()
            .find(|kind| attr.path().is_ident(kind.name))
        else {
            continue;
        };
        if !matches!(attr.meta, syn::Meta::Path(_)) {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!("#[{}] does not take arguments", kind.name),
            ));
        }
        if let Some(previous) = found {
            if previous.name != kind.name {
                return Err(syn::Error::new(
                    field.ident.as_ref().expect("named field").span(),
                    conflict_message(previous.name, kind.name),
                ));
            }
        }
        found = Some(kind);
    }
    Ok(found)
}

/// The three original pairs keep their wording so existing compile-fail output stays stable.
fn conflict_message(first: &str, second: &str) -> String {
    match (first, second) {
        ("prop", "event") | ("event", "prop") => {
            "a field cannot be both #[prop] and #[event]".to_string()
        }
        ("prop", "slot") | ("slot", "prop") => {
            "a field cannot be both #[prop] and #[slot]".to_string()
        }
        ("event", "slot") | ("slot", "event") => {
            "a field cannot be both #[event] and #[slot]".to_string()
        }
        _ => format!("a field cannot be both #[{first}] and #[{second}]"),
    }
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
        .filter(|attr| {
            !KIND_ATTRS
                .iter()
                .any(|kind| attr.path().is_ident(kind.name))
        })
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
        FieldKind::Slot(_) => quote! {
            #(#attrs)*
            #vis #name: ::cappy::Slot<Self, #ty>,
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
        FieldKind::Event => quote!(#name: ::cappy::Listener::empty(cx)),
        FieldKind::Slot(index) => quote!(#name: ::cappy::Slot::new(#index)),
        FieldKind::Prop if default_value => {
            quote!(
                #name: <#ty as ::cappy::IntoReactive>::into_prop_default(
                    <#ty as ::core::default::Default>::default(),
                    cx,
                )
            )
        }
        FieldKind::State if default_value => {
            quote!(#name: cx.reactive(<#ty as ::core::default::Default>::default()))
        }
        FieldKind::State | FieldKind::Prop => quote!(#name: cx.reactive(#name)),
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
        FieldKind::State | FieldKind::Slot(_) => None,
        FieldKind::Prop => Some(quote! {
            #(#docs)*
            #[must_use]
            pub fn #name<__Value, __Marker>(mut self, value: __Value) -> Self
            where
                __Value: ::cappy::PropInput<#ty, __Marker>,
            {
                let previous = self.#name.clone();
                ::cappy::PropInput::apply(value, &mut self.#name);
                if previous != self.#name {
                    ::cappy::RetirePlaceholder::dispose_if_placeholder(previous);
                }
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

/// `open` is what `ui!` `@Type` calls. State and props start from [`Default`].
fn open_method(
    generics: &Generics,
    _name: &syn::Ident,
    fields: &[&Field],
    kinds: &[FieldKind],
) -> TokenStream {
    let _ = generics;
    let bounds: Vec<_> = fields
        .iter()
        .zip(kinds)
        .filter_map(|(field, kind)| {
            if matches!(kind, FieldKind::State | FieldKind::Prop) {
                let ty = &field.ty;
                Some(quote!(#ty: ::core::default::Default))
            } else {
                None
            }
        })
        .collect();
    let where_clause = if bounds.is_empty() {
        quote!()
    } else {
        quote!(where #(#bounds,)*)
    };
    let inits = fields
        .iter()
        .zip(kinds)
        .map(|(field, kind)| field_init(field, kind, true));
    quote! {
        pub fn open(cx: &::cappy::Context) -> Self
        #where_clause
        {
            Self {
                context: cx.clone(),
                #(#inits,)*
            }
        }
    }
}
