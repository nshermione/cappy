//! `stylesheet!` defines an enum and the style rules for its variants in one place.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, Token, Visibility, braced};

use crate::class_name::class_name;

const CLASS_ATTRIBUTE: &str = "class";

pub struct Input {
    visibility: Visibility,
    name: Ident,
    classes: Vec<ClassDecl>,
    pseudos: Vec<Pseudo>,
}

struct ClassDecl {
    attributes: Vec<Attribute>,
    name: Ident,
    items: Vec<BodyItem>,
}

struct Pseudo {
    class: Ident,
    kind: PseudoKind,
    properties: Vec<Property>,
}

#[derive(Clone)]
struct Property {
    key: Ident,
    value: Expr,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PseudoKind {
    Hover,
    Active,
}

impl PseudoKind {
    fn suffix(self) -> &'static str {
        match self {
            Self::Hover => ":hover",
            Self::Active => ":active",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Active => "active",
        }
    }
}

enum BodyItem {
    Include(Ident),
    Property(Property),
}

/// A resolved rule: links keep included class names as real paths, then setters run in order.
struct Rule {
    links: Vec<Ident>,
    properties: Vec<Property>,
}

impl syn::parse::Parse for Input {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let visibility = input.parse()?;
        input.parse::<Token![enum]>()?;
        let name = input.parse()?;
        let content;
        braced!(content in input);
        let mut classes = Vec::new();
        while !content.is_empty() {
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
                continue;
            }
            classes.push(content.parse()?);
        }
        let mut pseudos = Vec::new();
        while !input.is_empty() {
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                continue;
            }
            pseudos.push(input.parse()?);
        }
        Ok(Self {
            visibility,
            name,
            classes,
            pseudos,
        })
    }
}

impl syn::parse::Parse for ClassDecl {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            attributes: input.call(Attribute::parse_outer)?,
            name: input.parse()?,
            items: parse_braced(input)?,
        })
    }
}

impl syn::parse::Parse for Pseudo {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let class = input.parse()?;
        input.parse::<Token![:]>()?;
        let kind_name: Ident = input.parse()?;
        let kind = match kind_name.to_string().as_str() {
            "hover" => PseudoKind::Hover,
            "active" => PseudoKind::Active,
            _ => {
                return Err(syn::Error::new(
                    kind_name.span(),
                    "expected `hover` or `active`",
                ));
            }
        };
        let items: Vec<BodyItem> = parse_braced(input)?;
        let mut properties = Vec::new();
        for item in items {
            match item {
                BodyItem::Include(name) => {
                    return Err(syn::Error::new(
                        name.span(),
                        "hover and active blocks only set properties",
                    ));
                }
                BodyItem::Property(property) => properties.push(property),
            }
        }
        Ok(Self {
            class,
            kind,
            properties,
        })
    }
}

impl syn::parse::Parse for BodyItem {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(Token![@]) {
            input.parse::<Token![@]>()?;
            let keyword: Ident = input.parse()?;
            if keyword != "include" {
                return Err(syn::Error::new(keyword.span(), "expected `include`"));
            }
            return Ok(Self::Include(input.parse()?));
        }
        let key = input.parse()?;
        input.parse::<Token![:]>()?;
        Ok(Self::Property(Property {
            key,
            value: input.parse()?,
        }))
    }
}

fn parse_braced<T: syn::parse::Parse>(input: ParseStream<'_>) -> syn::Result<Vec<T>> {
    let content;
    braced!(content in input);
    let items = Punctuated::<T, Token![,]>::parse_terminated(&content)?;
    Ok(items.into_iter().collect())
}

pub fn expand(input: &Input) -> syn::Result<TokenStream> {
    reject_duplicates(input)?;

    let mut rules = Vec::new();
    for class in &input.classes {
        let resolved = resolve_class(class, &input.classes, &mut Vec::new())?;
        let selector = class_name(&class.name, &class.attributes)?;
        rules.push(emit_rule(&input.name, selector, &resolved));
    }

    let mut seen_pseudos = Vec::new();
    for pseudo in &input.pseudos {
        let Some(class) = input
            .classes
            .iter()
            .find(|class| class.name == pseudo.class)
        else {
            return Err(syn::Error::new(
                pseudo.class.span(),
                format!("class `{}` is not defined in this sheet", pseudo.class),
            ));
        };
        let key = (pseudo.class.to_string(), pseudo.kind);
        if seen_pseudos.contains(&key) {
            return Err(syn::Error::new(
                pseudo.class.span(),
                format!(
                    "`{}:{}` is already defined",
                    pseudo.class,
                    pseudo.kind.label()
                ),
            ));
        }
        seen_pseudos.push(key);
        let selector = format!(
            "{}{}",
            class_name(&class.name, &class.attributes)?,
            pseudo.kind.suffix()
        );
        let base = resolve_class(class, &input.classes, &mut Vec::new())?;
        let mut properties = base.properties;
        properties.extend(pseudo.properties.iter().cloned());
        let link = &pseudo.class;
        let body = emit_rule(
            &input.name,
            selector,
            &Rule {
                links: base.links,
                properties,
            },
        );
        let enum_name = &input.name;
        rules.push(quote! {
            let _ = #enum_name::#link;
            #body
        });
    }

    let visibility = &input.visibility;
    let name = &input.name;
    let variants = input.classes.iter().map(|class| {
        let ident = &class.name;
        let attributes = class
            .attributes
            .iter()
            .filter(|attribute| !attribute.path().is_ident(CLASS_ATTRIBUTE));
        quote! { #(#attributes)* #ident }
    });
    let variant_names: Vec<_> = input.classes.iter().map(|class| &class.name).collect();
    let arms = input.classes.iter().map(|class| {
        let ident = &class.name;
        let selector = class_name(ident, &class.attributes).expect("selector was validated");
        quote! { #name::#ident => #selector }
    });

    Ok(quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #visibility enum #name {
            #(#variants,)*
        }

        impl #name {
            /// Every class, in declaration order.
            pub const ALL: &'static [#name] = &[#(#name::#variant_names,)*];

            /// The class name as used in themes and `Element::class`.
            pub const fn as_str(&self) -> &'static str {
                match self {
                    #(#arms,)*
                }
            }

            /// Rules for these classes. Merging this into a theme makes them available on every node.
            pub fn sheet() -> ::std::sync::Arc<Stylesheet> {
                let mut sheet = Stylesheet::default();
                #(#rules)*
                ::std::sync::Arc::new(sheet)
            }

            /// Merges this sheet into `theme`.
            pub fn apply(theme: Theme) -> Theme {
                theme.with_sheet(&#name::sheet())
            }
        }

        impl ::core::fmt::Display for #name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::core::convert::AsRef<str> for #name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    })
}

fn reject_duplicates(input: &Input) -> syn::Result<()> {
    let mut names = Vec::new();
    let mut selectors = Vec::new();
    for class in &input.classes {
        let selector = class_name(&class.name, &class.attributes)?;
        if names.iter().any(|name: &Ident| name == &class.name) {
            return Err(syn::Error::new(
                class.name.span(),
                format!("duplicate class `{}`", class.name),
            ));
        }
        if selectors
            .iter()
            .any(|existing: &String| existing == &selector)
        {
            return Err(syn::Error::new(
                class.name.span(),
                format!("duplicate class name `{selector}`"),
            ));
        }
        names.push(class.name.clone());
        selectors.push(selector);
    }
    Ok(())
}

fn resolve_class(
    class: &ClassDecl,
    classes: &[ClassDecl],
    stack: &mut Vec<String>,
) -> syn::Result<Rule> {
    stack.push(class.name.to_string());
    let mut rule = Rule {
        links: Vec::new(),
        properties: Vec::new(),
    };
    for item in &class.items {
        match item {
            BodyItem::Include(target) => {
                if stack.iter().any(|seen| seen == &target.to_string()) {
                    return Err(syn::Error::new(
                        target.span(),
                        format!("`{target}` is included in a cycle"),
                    ));
                }
                let Some(included) = classes.iter().find(|candidate| candidate.name == *target)
                else {
                    return Err(syn::Error::new(
                        target.span(),
                        format!("class `{target}` is not defined in this sheet"),
                    ));
                };
                rule.links.push(target.clone());
                let nested = resolve_class(included, classes, stack)?;
                rule.links.extend(nested.links);
                rule.properties.extend(nested.properties);
            }
            BodyItem::Property(property) => rule.properties.push(property.clone()),
        }
    }
    stack.pop();
    Ok(rule)
}

fn emit_rule(enum_name: &Ident, selector: String, rule: &Rule) -> TokenStream {
    let links = rule.links.iter().map(|name| {
        quote! { let _ = #enum_name::#name; }
    });
    let setters = rule.properties.iter().map(|property| {
        let key = &property.key;
        let value = &property.value;
        quote! { .#key(#value) }
    });
    quote! {
        #(#links)*
        sheet.set_class(#selector, StyleRefinement::default() #(#setters)*);
    }
}
