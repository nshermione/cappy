use cappy_macros::component;

#[component]
struct Badge {
    #[slots]
    label: String,
}

fn main() {}
