use cappy_macros::ClassName;

#[derive(ClassName)]
enum Class {
    Card,
    Sized(u32),
}

fn main() {}
