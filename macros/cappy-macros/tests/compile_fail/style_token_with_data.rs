use cappy_macros::style_token;

#[style_token(f32)]
enum Depth {
    Near(f32),
}

fn main() {}
