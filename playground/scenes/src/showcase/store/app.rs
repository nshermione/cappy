//! App-wide state that other components read and write. Debug is one of those values: the header
//! toggles it, and the shell shows the panel.

use cappy::prelude::*;

#[derive(Copy)]
#[store]
pub struct AppStore {
    pub debug: bool,
}
