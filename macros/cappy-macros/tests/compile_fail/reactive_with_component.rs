#![allow(unused_imports)]

use cappy_macros::{component, reactive};

#[reactive]
#[component]
struct Badge {
    text: String,
}

fn main() {}
