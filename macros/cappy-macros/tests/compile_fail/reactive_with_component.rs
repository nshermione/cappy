#![allow(unused_imports)]

use cappy_macros::{component, store};

#[store]
#[component]
struct Badge {
    text: String,
}

fn main() {}
