//! `#[style_token(Type)]` turns a fieldless enum into a style token.
//!
//! The enum stays the name used in styles. A `{Enum}Values` struct holds one value per variant,
//! and `{Enum}::patch()` builds a partial overwrite of those values.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Error, Fields, Item, ItemEnum, Token, Type, Variant};

struct Args {
    value: Type,
    field: Option<Ident>,
}

impl Parse for Args {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error("expected the value type, for example #[style_token(f32)]"));
        }
        let value = input.parse()?;
        let field = if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        if !input.is_empty() {
            return Err(input.error("expected #[style_token(Type)] or #[style_token(Type, field)]"));
        }
        Ok(Self { value, field })
    }
}

struct Slot {
    variant: Ident,
    field: Ident,
    index: u8,
}

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args: Args = syn::parse2(attr)?;
    let parsed: Item = syn::parse2(item)?;
    let Item::Enum(mut item) = parsed else {
        return Err(Error::new(
            Span::call_site(),
            "#[style_token] only supports enums",
        ));
    };
    if !item.generics.params.is_empty() {
        return Err(Error::new(
            item.generics.span(),
            "a style token enum cannot have generic parameters",
        ));
    }
    let slots = slots(&item)?;
    item.attrs.insert(
        0,
        syn::parse_quote!(#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]),
    );

    let enum_name = &item.ident;
    let values_name = Ident::new(&format!("{enum_name}Values"), enum_name.span());
    let patch_name = Ident::new(&format!("{enum_name}Patch"), enum_name.span());
    let value = &args.value;
    let vis = &item.vis;

    let field_idents: Vec<&Ident> = slots.iter().map(|slot| &slot.field).collect();
    let setters = slots.iter().map(|slot| {
        let field = &slot.field;
        quote! {
            #[must_use]
            pub fn #field(mut self, value: #value) -> Self {
                self.#field = Some(value);
                self
            }
        }
    });
    let apply_fields = slots.iter().map(|slot| {
        let field = &slot.field;
        quote! {
            if let Some(value) = self.#field {
                values.#field = value;
            }
        }
    });
    let get_arms = slots.iter().map(|slot| {
        let variant = &slot.variant;
        let field = &slot.field;
        quote! { #enum_name::#variant => self.#field }
    });
    let set_arms = slots.iter().map(|slot| {
        let variant = &slot.variant;
        let field = &slot.field;
        quote! { #enum_name::#variant => self.#field = value }
    });
    let index_arms = slots.iter().map(|slot| {
        let variant = &slot.variant;
        let index = slot.index;
        quote! { #enum_name::#variant => #index }
    });
    let from_arms = slots.iter().map(|slot| {
        let variant = &slot.variant;
        let index = slot.index;
        quote! { #index => #enum_name::#variant }
    });

    let wiring = if let Some(field) = &args.field {
        quote! {
            impl StyleToken for #enum_name {
                type Value = #value;

                fn index(self) -> u8 {
                    match self {
                        #(#index_arms,)*
                    }
                }

                fn from_index(index: u8) -> Self {
                    match index {
                        #(#from_arms,)*
                        _ => panic!("style token index is out of range"),
                    }
                }

                fn read(self, tokens: &Tokens) -> Self::Value {
                    tokens.#field.get(self)
                }

                fn stored(_tokens: &Tokens) -> bool {
                    true
                }
            }

            impl PatchList for #patch_name {
                fn apply(self, tokens: &mut Tokens) {
                    self.apply_to(&mut tokens.#field);
                }
            }
        }
    } else {
        quote! {
            impl ::cappy::StyleToken for #enum_name {
                type Value = #value;

                fn index(self) -> u8 {
                    match self {
                        #(#index_arms,)*
                    }
                }

                fn from_index(index: u8) -> Self {
                    match index {
                        #(#from_arms,)*
                        _ => panic!("style token index is out of range"),
                    }
                }

                fn read(self, tokens: &::cappy::Tokens) -> Self::Value {
                    tokens.read_custom::<#enum_name>(self)
                }

                fn stored(tokens: &::cappy::Tokens) -> bool {
                    tokens.has_custom::<#enum_name>()
                }
            }

            impl ::cappy::CustomToken for #enum_name {
                type Value = #value;
                type Values = #values_name;
                type Patch = #patch_name;

                fn read_values(self, values: &Self::Values) -> Self::Value {
                    values.get(self)
                }

                fn apply_patch(patch: Self::Patch, values: &mut Self::Values) {
                    patch.apply_to(values);
                }
            }

            impl ::cappy::PatchList for #patch_name {
                fn apply(self, tokens: &mut ::cappy::Tokens) {
                    tokens.apply_custom::<#enum_name>(self);
                }
            }
        }
    };

    Ok(quote! {
        #item

        #[derive(Clone, Debug, PartialEq, Default)]
        #vis struct #values_name {
            #(#vis #field_idents: #value,)*
        }

        impl #values_name {
            pub fn get(&self, token: #enum_name) -> #value {
                match token {
                    #(#get_arms,)*
                }
            }

            pub fn set(&mut self, token: #enum_name, value: #value) {
                match token {
                    #(#set_arms,)*
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, Default)]
        #vis struct #patch_name {
            #(#field_idents: Option<#value>,)*
        }

        impl #enum_name {
            #[must_use]
            pub fn patch() -> #patch_name {
                #patch_name::default()
            }
        }

        impl #patch_name {
            #(#setters)*

            pub fn apply_to(self, values: &mut #values_name) {
                #(#apply_fields)*
            }
        }

        #wiring
    })
}

fn slots(item: &ItemEnum) -> syn::Result<Vec<Slot>> {
    if item.variants.is_empty() {
        return Err(Error::new(
            item.ident.span(),
            "a style token enum needs at least one variant",
        ));
    }
    if item.variants.len() > usize::from(u8::MAX) {
        return Err(Error::new(
            item.ident.span(),
            "a style token enum can have at most 256 variants",
        ));
    }
    item.variants
        .iter()
        .enumerate()
        .map(|(index, variant)| slot(variant, index))
        .collect()
}

fn slot(variant: &Variant, index: usize) -> syn::Result<Slot> {
    if variant.discriminant.is_some() {
        return Err(Error::new(
            variant.span(),
            "style token variants cannot set an explicit discriminant",
        ));
    }
    if !matches!(variant.fields, Fields::Unit) {
        return Err(Error::new(
            variant.span(),
            "style token variants cannot hold data",
        ));
    }
    let field = Ident::new(
        &snake_case(&variant.ident.to_string()),
        variant.ident.span(),
    );
    Ok(Slot {
        variant: variant.ident.clone(),
        field,
        index: u8::try_from(index).expect("variant count is checked"),
    })
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
