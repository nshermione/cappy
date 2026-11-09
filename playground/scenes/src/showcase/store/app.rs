//! The app store: which tab is open. One value per context, same as the counter.

use cappy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Default, Reactive)]
pub enum Tab {
    #[default]
    Counter,
    Tiles,
    Checklist,
    Paint,
    Motion,
}

#[derive(Copy)]
#[store]
pub struct App {
    pub tab: Tab,
}

impl App {
    pub fn select(&self, tab: Tab) {
        self.tab.set(tab);
    }
}
