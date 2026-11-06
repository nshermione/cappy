use cappy_macros::stylesheet;

stylesheet! {
    enum Class {
        Badge {
            @include Missing,
        }
    }
}

fn main() {}
