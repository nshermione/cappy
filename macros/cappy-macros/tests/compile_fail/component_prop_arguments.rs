use cappy_macros::component;

#[component]
struct Badge {
    #[prop(skip)]
    label: String,
}

fn main() {}
