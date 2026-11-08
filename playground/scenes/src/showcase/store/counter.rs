//! Shared counter. `use_store(Counter)` is one value in a context. `use_local_store(Counter)` is a
//! separate instance whose fields start at their defaults. `new` builds one from explicit values.

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
