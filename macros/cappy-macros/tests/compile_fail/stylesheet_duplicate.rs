use cappy_macros::stylesheet;

stylesheet! {
    enum Class {
        Badge { background: 1 }
        Badge {
            @include Badge,
        }
    }
}

fn main() {}
