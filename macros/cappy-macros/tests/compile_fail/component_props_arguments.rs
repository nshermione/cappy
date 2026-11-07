use cappy_macros::component;

#[component]
struct Badge {
    #[props(skip)]
    label: String,
}

fn main() {}
