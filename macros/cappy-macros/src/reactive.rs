use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Field, Fields, Generics, Ident, spanned::Spanned};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
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
    let generics = with_into_reactive(&input.generics);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let reactive_fields = fields.iter().copied().map(reactive_field);
    let field_args = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().expect("named field");
        let ty = &field.ty;
        quote!(#field_name: #ty)
    });
    let field_inits = fields.iter().map(|field| {
        let field_name = field.ident.as_ref().expect("named field");
        quote!(#field_name: ::cappy::reactive(#field_name))
    });
    let copy_types: Vec<TokenStream> = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            quote!(<#ty as ::cappy::IntoReactive>::Output)
        })
        .collect();
    let tracked = tracked_impls(name, &generics, &copy_types);

    Ok(quote! {
        #(#attrs)*
        #vis struct #name #impl_generics #where_clause {
            #(#reactive_fields)*
        }

        impl #impl_generics #name #type_generics #where_clause {
            pub fn new(#(#field_args),*) -> Self {
                Self {
                    #(#field_inits,)*
                }
            }
        }

        #tracked
    })
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

/// `IntoReactive`, `Copy` and `Clone` for a tracked struct. Each type in `copy_types` must be
/// [`Copy`](core::marker::Copy) for the copy impl to apply.
pub(crate) fn tracked_impls(
    name: &Ident,
    generics: &Generics,
    copy_types: &[TokenStream],
) -> TokenStream {
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let mut copy_generics = generics.clone();
    if !copy_types.is_empty() {
        let predicates = &mut copy_generics.make_where_clause().predicates;
        for ty in copy_types {
            predicates.push(syn::parse_quote!(#ty: ::core::marker::Copy));
        }
    }
    let (copy_impl_generics, copy_type_generics, copy_where) = copy_generics.split_for_impl();
    quote! {
        impl #impl_generics ::cappy::IntoReactive for #name #type_generics #where_clause {
            type Output = Self;

            fn into_reactive(self) -> Self::Output {
                self
            }
        }

        impl #copy_impl_generics ::core::marker::Copy for #name #copy_type_generics #copy_where {}

        impl #copy_impl_generics ::core::clone::Clone for #name #copy_type_generics #copy_where {
            fn clone(&self) -> Self {
                *self
            }
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
