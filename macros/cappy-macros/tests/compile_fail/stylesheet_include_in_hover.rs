use cappy_macros::stylesheet;

stylesheet! {
    enum Class {
        Badge {
            :hover {
                @include Badge,
            }
        }
    }
}

fn main() {}
