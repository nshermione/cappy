//! Copies `self.field` out of a `ui!` closure so the closure does not capture `&self`.
//!
//! State and prop fields are [`Copy`] handles. The copy sits outside the closure, so
//! `text(:format!("{}", self.label))` stays `'static`.
//!
//! [`expand_view`](expand_view) gives a `'static` element closure the caller's `cx` without
//! borrowing it.

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

/// `with_context ; context_ident ; move || body`
///
/// `context_ident` is the name passed to `ui!((context) { ... })`. `show` and `if` use this.
/// `for` uses [`expand_for_view`](expand_for_view): its row closure has to be an argument of
/// `for_each`, or Rust will not infer the row type.
pub fn expand_view(input: TokenStream) -> TokenStream {
    let (with_context, context, closure) = match split_named(input) {
        Ok(parts) => parts,
        Err(message) => return quote!(::core::compile_error!(#message);),
    };
    let (args, body) = match split_closure(closure) {
        Ok(parts) => parts,
        Err(message) => return quote!(::core::compile_error!(#message);),
    };
    if !args.is_empty() {
        return quote!(::core::compile_error!("ui view closure cannot take arguments"););
    }
    finish_view(with_context, context, body)
}

/// `for_each ; Context ; context_ident ; read ; key ; move |item| body`
pub fn expand_for_view(input: TokenStream) -> TokenStream {
    let (for_each, context_path, context, read, key, closure) = match split_for_view(input) {
        Ok(parts) => parts,
        Err(message) => return quote!(::core::compile_error!(#message);),
    };
    let (args, body) = match split_closure(closure) {
        Ok(parts) => parts,
        Err(message) => return quote!(::core::compile_error!(#message);),
    };
    let mut captures = Vec::new();
    let rewritten = rewrite(body, &mut captures);
    let captured = Ident::new("__cappy_cx", Span::mixed_site());
    let lets = field_lets(&captures);
    quote! {
        {
            #lets
            let #captured = <#context_path as ::core::clone::Clone>::clone(&#context);
            #for_each(
                #read,
                #key,
                move |#args| {
                    #[allow(unused_variables)]
                    let #context = &#captured;
                    #rewritten
                },
            )
        }
    }
}

fn finish_view(with_context: TokenStream, context: Ident, body: TokenStream) -> TokenStream {
    let mut captures = Vec::new();
    let rewritten = rewrite(body, &mut captures);
    let lets = field_lets(&captures);
    let call = quote! {
        #with_context(
            &#context,
            #[allow(unused_variables)]
            move |#context| #rewritten,
        )
    };
    if captures.is_empty() {
        return call;
    }
    quote! {
        {
            #lets
            #call
        }
    }
}

fn field_lets(captures: &[Capture]) -> TokenStream {
    let lets = captures.iter().map(|capture| {
        let receiver = Ident::new("self", capture.self_span);
        let field = &capture.field;
        let local = &capture.local;
        quote!(let #local = #receiver.#field;)
    });
    quote!(#(#lets)*)
}

/// `path ; context_ident ; closure`
fn split_named(input: TokenStream) -> Result<(TokenStream, Ident, TokenStream), String> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let semis = top_semis(&tokens);
    if semis.is_empty() {
        return Err("ui view is missing a path".into());
    }
    let path: TokenStream = tokens[..semis[0]].iter().cloned().collect();
    if path.is_empty() {
        return Err("ui view is missing a path".into());
    }
    let Some(TokenTree::Ident(context)) = tokens.get(semis[0] + 1).cloned() else {
        return Err("ui view is missing the context name".into());
    };
    if semis.get(1) != Some(&(semis[0] + 2)) {
        return Err("ui view is missing the closure".into());
    }
    let closure: TokenStream = tokens[semis[1] + 1..].iter().cloned().collect();
    Ok((path, context, closure))
}

fn split_for_view(
    input: TokenStream,
) -> Result<
    (
        TokenStream,
        TokenStream,
        Ident,
        TokenStream,
        TokenStream,
        TokenStream,
    ),
    String,
> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let semis = top_semis(&tokens);
    if semis.len() < 5 {
        return Err("ui for view is missing a path".into());
    }
    let for_each: TokenStream = tokens[..semis[0]].iter().cloned().collect();
    let context_path: TokenStream = tokens[semis[0] + 1..semis[1]].iter().cloned().collect();
    if for_each.is_empty() || context_path.is_empty() {
        return Err("ui for view is missing a path".into());
    }
    let Some(TokenTree::Ident(context)) = tokens.get(semis[1] + 1).cloned() else {
        return Err("ui for view is missing the context name".into());
    };
    if semis.get(2) != Some(&(semis[1] + 2)) {
        return Err("ui for view is missing the read closure".into());
    }
    let read: TokenStream = tokens[semis[2] + 1..semis[3]].iter().cloned().collect();
    let key: TokenStream = tokens[semis[3] + 1..semis[4]].iter().cloned().collect();
    let closure: TokenStream = tokens[semis[4] + 1..].iter().cloned().collect();
    if read.is_empty() || key.is_empty() || closure.is_empty() {
        return Err("ui for view is missing the read, key, or row closure".into());
    }
    Ok((for_each, context_path, context, read, key, closure))
}

fn top_semis(tokens: &[TokenTree]) -> Vec<usize> {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| is_semi(token))
        .map(|(index, _)| index)
        .collect()
}

fn is_pipe(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == '|')
}

fn is_semi(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == ';')
}

fn is_dot(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == '.')
}
