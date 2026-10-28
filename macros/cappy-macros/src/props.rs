use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Field, Fields, GenericArgument, PathArguments, Type,
    spanned::Spanned,
};

const ATTRIBUTE: &str = "props";
const SKIP: &str = "skip";

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = named_fields(input)?;
    let mut setters = Vec::new();
    for field in fields {
        if !is_skipped(field)? {
            setters.push(setter(field));
        }
    }
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #name #type_generics #where_clause {
            #(#setters)*
        }
    })
}

fn named_fields(input: &DeriveInput) -> syn::Result<impl Iterator<Item = &Field>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Props)] only supports structs with named fields",
        ));
    };
    match &data.fields {
        Fields::Named(fields) => Ok(fields.named.iter()),
        other => Err(syn::Error::new(
            other.span(),
            "#[derive(Props)] needs named fields to name the setters after",
        )),
    }
}

fn is_skipped(field: &Field) -> syn::Result<bool> {
    let mut skipped = false;
    for attribute in field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident(ATTRIBUTE))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident(SKIP) {
                skipped = true;
                Ok(())
            } else {
                Err(meta.error("unknown props option; expected `skip`"))
            }
        })?;
    }
    Ok(skipped)
}

fn setter(field: &Field) -> TokenStream {
    let name = field
        .ident
        .as_ref()
        .expect("named fields always have identifiers");
    let docs: Vec<&Attribute> = field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .collect();
    match option_inner(&field.ty) {
        Some(inner) => quote! {
            #(#docs)*
            #[must_use]
            pub fn #name(mut self, value: impl ::core::convert::Into<#inner>) -> Self {
                self.#name = ::core::option::Option::Some(value.into());
                self
            }
        },
        None => {
            let ty = &field.ty;
            quote! {
                #(#docs)*
                #[must_use]
                pub fn #name(mut self, value: impl ::core::convert::Into<#ty>) -> Self {
                    self.#name = value.into();
                    self
                }
            }
        }
    }
}

/// `T` when `ty` is spelled `Option<T>`.
fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    match arguments.args.first()? {
        GenericArgument::Type(inner) if arguments.args.len() == 1 => Some(inner),
        _ => None,
    }
}
