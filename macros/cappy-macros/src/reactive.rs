use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Field, Fields, spanned::Spanned};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = named_fields(input)?;
    let vis = &input.vis;
    let name = &input.ident;
    let attrs = &input.attrs;
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(syn::parse_quote!(::cappy::IntoReactive));
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
        quote!(#field_name: ::cappy::reactive(#field_name))
    });
    let mut copy_generics = generics.clone();
    if !fields.is_empty() {
        let predicates = &mut copy_generics.make_where_clause().predicates;
        for field in &fields {
            let ty = &field.ty;
            predicates.push(syn::parse_quote! {
                <#ty as ::cappy::IntoReactive>::Output: ::core::marker::Copy
            });
        }
    }
    let (copy_impl_generics, copy_type_generics, copy_where) = copy_generics.split_for_impl();

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
    })
}

fn named_fields(input: &DeriveInput) -> syn::Result<Vec<&Field>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[reactive] only supports structs with named fields",
        ));
    };
    match &data.fields {
        Fields::Named(fields) => Ok(fields.named.iter().collect()),
        other => Err(syn::Error::new(
            other.span(),
            "#[reactive] only supports structs with named fields",
        )),
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
