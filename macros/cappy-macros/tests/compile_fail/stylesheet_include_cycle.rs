use cappy_macros::stylesheet;

stylesheet! {
    enum Class {
        A {
            @include B,
        }
        B {
            @include A,
        }
    }
}

fn main() {}
