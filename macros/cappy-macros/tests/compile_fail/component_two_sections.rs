use cappy_macros::component;

#[component]
struct Badge {
    #[props]
    #[events]
    label: String,
}

fn main() {}
