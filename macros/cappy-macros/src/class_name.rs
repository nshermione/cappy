use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, Variant};

const ATTRIBUTE: &str = "class";
const NAME: &str = "name";

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(ClassName)] only supports enums",
        ));
    };
    let mut arms = Vec::new();
    let mut variants = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.fields,
                "#[derive(ClassName)] variants cannot carry data",
            ));
        }
        let ident = &variant.ident;
        let class = class_name_of(variant)?;
        arms.push(quote! { Self::#ident => #class });
        variants.push(quote! { Self::#ident });
    }

    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #name #type_generics #where_clause {
            /// Every class, in declaration order.
            pub const ALL: &'static [Self] = &[#(#variants),*];

            /// The class name as used in themes and `Element::class`.
            pub const fn as_str(&self) -> &'static str {
                match self {
                    #(#arms,)*
                }
            }
        }

        impl #impl_generics ::core::fmt::Display for #name #type_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl #impl_generics ::core::convert::AsRef<str> for #name #type_generics #where_clause {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    })
}

fn class_name_of(variant: &Variant) -> syn::Result<String> {
    let mut explicit = None;
    for attribute in variant
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident(ATTRIBUTE))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident(NAME) {
                let value: LitStr = meta.value()?.parse()?;
                explicit = Some(value.value());
                Ok(())
            } else {
                Err(meta.error("unknown class option; expected `name = \"...\"`"))
            }
        })?;
    }
    Ok(explicit.unwrap_or_else(|| kebab_case(&variant.ident.to_string())))
}

/// `BtnPrimary` becomes `btn-primary`; a dash goes before each capital that follows a lowercase
/// letter or a digit.
fn kebab_case(identifier: &str) -> String {
    let mut result = String::with_capacity(identifier.len() + 4);
    let mut previous: Option<char> = None;
    for character in identifier.chars() {
        let after_word = previous.is_some_and(|prev| prev.is_lowercase() || prev.is_ascii_digit());
        if character.is_uppercase() && after_word {
            result.push('-');
        }
        result.extend(character.to_lowercase());
        previous = Some(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::kebab_case;

    #[test]
    fn converts_pascal_case_to_kebab_case() {
        assert_eq!(kebab_case("Card"), "card");
        assert_eq!(kebab_case("BtnPrimary"), "btn-primary");
        assert_eq!(kebab_case("H1"), "h1");
        assert_eq!(kebab_case("Grid3Col"), "grid3-col");
    }
}
