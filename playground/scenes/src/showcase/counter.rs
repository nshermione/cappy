//! Shared counter. `singleton` is one value on this thread; `new` is a separate instance.

use cappy::prelude::*;

#[reactive(singleton)]
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
