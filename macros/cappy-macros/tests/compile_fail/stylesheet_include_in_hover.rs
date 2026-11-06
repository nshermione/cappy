use cappy_macros::stylesheet;

stylesheet! {
    enum Class {
        Badge {}
    }
    Badge:hover {
        @include Badge,
    }
}

fn main() {}
