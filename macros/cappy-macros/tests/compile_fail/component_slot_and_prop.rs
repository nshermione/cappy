use cappy_macros::component;

#[component]
struct Badge {
    #[prop]
    #[slot]
    body: Element,
}

fn main() {}
