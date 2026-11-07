use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Field, Fields, Generics, Ident, spanned::Spanned};

pub fn expand(input: &DeriveInput, singleton: bool) -> syn::Result<TokenStream> {
    if let Some(attr) = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("component"))
    {
        return Err(syn::Error::new(
            attr.path().span(),
            "#[component] already tracks its fields; remove #[reactive]",
        ));
    }
    let fields = named_fields(input, "#[reactive]")?;
    let vis = &input.vis;
    let name = &input.ident;
    let attrs = &input.attrs;
    let mut generics = with_into_reactive(&input.generics);
    let copy_types: Vec<TokenStream> = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            quote!(<#ty as ::cappy::IntoReactive>::Output)
        })
        .collect();
    {
        let predicates = &mut generics.make_where_clause().predicates;
        for ty in &copy_types {
            predicates.push(syn::parse_quote!(#ty: ::core::clone::Clone));
        }
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let reactive_fields = fields.iter().copied().map(reactive_field);
    let field_args = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().expect("named field");
        let ty = &field.ty;
        quote!(#field_name: #ty)
    });
    let field_inits = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().expect("named field");
        quote!(#field_name: cx.reactive(#field_name))
    });
    let tracked = tracked_impls(name, &generics, &copy_types);
    let marker = singleton.then(|| singleton_marker(vis, name, &generics));
    let singleton = singleton.then(|| singleton_impl(name, &generics, &fields));

    Ok(quote! {
        #[derive(Clone)]
        #(#attrs)*
        #vis struct #name #impl_generics #where_clause {
            #(#reactive_fields)*
        }

        impl #impl_generics #name #type_generics #where_clause {
            pub fn new(cx: &::cappy::Context, #(#field_args),*) -> Self {
                Self {
                    #(#field_inits,)*
                }
            }

        }

        #marker

        #singleton

        #tracked
    })
}

/// Same name as the struct, in the value namespace, so `cx.singleton(Name)` can name it.
fn singleton_marker(vis: &syn::Visibility, name: &Ident, generics: &Generics) -> TokenStream {
    if generics.params.is_empty() {
        quote! {
            #[allow(non_upper_case_globals)]
            #vis const #name: ::cappy::SingletonOf<#name> = ::cappy::SingletonOf::new();
        }
    } else {
        let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
        quote! {
            #[allow(non_snake_case)]
            #vis fn #name #impl_generics() -> ::cappy::SingletonOf<#name #type_generics> #where_clause {
                ::cappy::SingletonOf::new()
            }
        }
    }
}

/// `cx.singleton(Type)` for `#[reactive(singleton)]`. The value lives in that context.
fn singleton_impl(name: &Ident, generics: &Generics, fields: &[&Field]) -> TokenStream {
    let mut generics = generics.clone();
    {
        let predicates = &mut generics.make_where_clause().predicates;
        for field in fields {
            let ty = &field.ty;
            predicates.push(syn::parse_quote!(#ty: ::core::default::Default));
        }
        predicates.push(syn::parse_quote!(Self: ::core::clone::Clone + 'static));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let defaults = fields
        .iter()
        .map(|_| quote!(::core::default::Default::default()));
    quote! {
        impl #impl_generics ::cappy::Singleton for #name #type_generics #where_clause {
            fn create(cx: &::cappy::Context) -> Self {
                Self::new(cx, #(#defaults),*)
            }
        }
    }
}

pub(crate) fn named_fields<'a>(
    input: &'a DeriveInput,
    macro_name: &str,
) -> syn::Result<Vec<&'a Field>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("{macro_name} only supports structs with named fields"),
        ));
    };
    match &data.fields {
        Fields::Named(fields) => Ok(fields.named.iter().collect()),
        other => Err(syn::Error::new(
            other.span(),
            format!("{macro_name} only supports structs with named fields"),
        )),
    }
}

pub(crate) fn with_into_reactive(generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(syn::parse_quote!(::cappy::IntoReactive));
    }
    generics
}

/// `IntoReactive` for a tracked struct. `copy_types` is unused; handles are [`Clone`](Clone), not
/// [`Copy`](Copy).
pub(crate) fn tracked_impls(
    name: &Ident,
    generics: &Generics,
    _copy_types: &[TokenStream],
) -> TokenStream {
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics ::cappy::IntoReactive for #name #type_generics #where_clause {
            type Output = Self;

            fn into_reactive(self, _context: &::cappy::Context) -> Self::Output {
                self
            }
        }

        impl #impl_generics ::cappy::RetirePlaceholder for #name #type_generics #where_clause {
            fn dispose_if_placeholder(self) {}
        }
    }
}

fn reactive_field(field: &Field) -> TokenStream {
    let vis = &field.vis;
    let name = field.ident.as_ref().expect("named field");
    let ty = &field.ty;
    let attrs = &field.attrs;
    quote! {
        #(#attrs)*
        #vis #name: <#ty as ::cappy::IntoReactive>::Output,
    }
}
