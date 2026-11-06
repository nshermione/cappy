#![allow(unused_imports)]

use cappy_macros::{component, reactive};

#[component]
#[reactive]
struct Badge {
    text: String,
}

fn main() {}
