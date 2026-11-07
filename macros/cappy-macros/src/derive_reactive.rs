use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, parse_quote, spanned::Spanned};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    if matches!(input.data, Data::Union(_)) {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Reactive)] only supports structs and enums",
        ));
    }
    if let Some(lifetime) = input.generics.lifetimes().next() {
        return Err(syn::Error::new(
            lifetime.span(),
            "#[derive(Reactive)] only supports 'static types",
        ));
    }

    let name = &input.ident;
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: 'static));
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::cappy::IntoReactive for #name #type_generics #where_clause {
            type Output = ::cappy::Reactive<Self>;

            fn into_reactive(self, context: &::cappy::Context) -> Self::Output {
                ::cappy::Reactive::new(context, self)
            }
        }
    })
}
