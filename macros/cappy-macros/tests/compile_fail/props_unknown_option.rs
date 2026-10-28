use cappy_macros::Props;

#[derive(Props)]
struct Button {
    #[props(rename = "text")]
    label: String,
}

fn main() {}
