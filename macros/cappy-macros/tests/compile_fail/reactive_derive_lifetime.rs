use cappy_macros::Reactive;

#[derive(Reactive)]
struct Borrowed<'a> {
    text: &'a str,
}

fn main() {}
