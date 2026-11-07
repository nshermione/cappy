//! Shared counter. `cx.use_store(Counter)` is one value in a context; `new` is a separate instance.

use cappy::prelude::*;

#[derive(Copy)]
#[store]
pub struct Counter {
    pub count: i64,
}

impl Counter {
    pub fn increment(&self) {
        self.count.update(|value| *value += 1);
    }

    pub fn decrement(&self) {
        self.count.update(|value| *value -= 1);
    }

    pub fn reset(&self) {
        self.count.set(0);
    }
}
