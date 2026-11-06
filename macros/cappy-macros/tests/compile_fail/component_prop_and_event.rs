use cappy_macros::component;

#[component]
struct Badge {
    #[prop]
    #[event]
    on_click: String,
}

fn main() {}
