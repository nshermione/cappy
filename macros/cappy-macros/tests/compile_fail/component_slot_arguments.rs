use cappy_macros::component;

#[component]
struct Badge {
    #[slot(name = "body")]
    body: Element,
}

fn main() {}
