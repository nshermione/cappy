//! Copies `self.field` out of a `ui!` closure so the closure does not capture `&self`.
//!
//! State and prop fields are [`Copy`] handles. The copy sits outside the closure, so
//! `text(:format!("{}", self.label))` stays `'static`.

use proc_macro2::{Group, Ident, Span, TokenStream, TokenTree};
use quote::quote;

struct Capture {
    self_span: Span,
    field: Ident,
    local: Ident,
}

pub fn expand(input: TokenStream) -> TokenStream {
    let (args, body) = match split_closure(input) {
        Ok(parts) => parts,
        Err(message) => return quote!(::core::compile_error!(#message);),
    };
    let mut captures = Vec::new();
    let rewritten = rewrite(body, &mut captures);
    if captures.is_empty() {
        return quote!(move | #args | #rewritten);
    }
    let lets = captures.iter().map(|capture| {
        let receiver = Ident::new("self", capture.self_span);
        let field = &capture.field;
        let local = &capture.local;
        quote!(let #local = #receiver.#field;)
    });
    quote! {
        {
            #(#lets)*
            move | #args | #rewritten
        }
    }
}

fn split_closure(input: TokenStream) -> Result<(TokenStream, TokenStream), String> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut index = 0;
    if matches!(tokens.first(), Some(TokenTree::Ident(ident)) if ident == "move") {
        index = 1;
    }
    if tokens.get(index).is_none_or(|token| !is_pipe(token)) {
        return Err("ui closure must start with `|`".into());
    }
    index += 1;
    let args_start = index;
    while tokens.get(index).is_some_and(|token| !is_pipe(token)) {
        index += 1;
    }
    if index >= tokens.len() {
        return Err("ui closure is missing the closing `|`".into());
    }
    let args = tokens[args_start..index].iter().cloned().collect();
    let body: TokenStream = tokens[index + 1..].iter().cloned().collect();
    if body.is_empty() {
        return Err("ui closure is missing a body".into());
    }
    Ok((args, body))
}

fn rewrite(input: TokenStream, captures: &mut Vec<Capture>) -> TokenStream {
    let mut output = TokenStream::new();
    let mut tokens = input.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Group(group) => {
                let mut nested = Group::new(group.delimiter(), rewrite(group.stream(), captures));
                nested.set_span(group.span());
                output.extend([TokenTree::Group(nested)]);
            }
            TokenTree::Ident(ident) if ident == "self" => {
                if tokens.peek().is_some_and(is_dot) {
                    let dot = tokens.next().unwrap();
                    if let Some(TokenTree::Ident(field)) = tokens.peek().cloned() {
                        tokens.next();
                        let local = capture(captures, ident.span(), field);
                        output.extend(quote!(#local));
                        continue;
                    }
                    output.extend([TokenTree::Ident(ident), dot]);
                    continue;
                }
                output.extend([TokenTree::Ident(ident)]);
            }
            other => output.extend([other]),
        }
    }
    output
}

fn capture(captures: &mut Vec<Capture>, self_span: Span, field: Ident) -> Ident {
    if let Some(existing) = captures.iter().find(|capture| capture.field == field) {
        return existing.local.clone();
    }
    let local = Ident::new(&format!("__cappy_{field}"), Span::mixed_site());
    captures.push(Capture {
        self_span,
        field,
        local: local.clone(),
    });
    local
}

fn is_pipe(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == '|')
}

fn is_dot(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == '.')
}
