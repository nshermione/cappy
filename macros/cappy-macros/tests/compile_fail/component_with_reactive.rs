#![allow(unused_imports)]

use cappy_macros::{component, store};

#[component]
#[store]
struct Badge {
    text: String,
}

fn main() {}
