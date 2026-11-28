//! One tab button. It shares the open tab with `App` and selects its own tab.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::super::style::{Class, class_when};
use super::app::Tab;

#[component]
pub(super) struct TabButton {
    #[props]
    selected: Tab,
    tab: Tab,
    title: String,
}

impl Component for TabButton {
    fn render(&self) -> Element {
        let selected = self.selected;
        let tab = self.tab.get();
        element! {
            div(
                class = ThemeClass::Btn,
                :class = class_when(selected.get() == tab, Class::TabActive),
                on_click = move |_| {
                    selected.set(tab);
                    start_motion_loops(tab);
                },
            ) {
                text(:self.title.get())
            }
        }
    }
}

fn start_motion_loops(tab: Tab) {
    if tab == Tab::Motion {
        Context::current().animate(super::super::motion::spawn_loops);
    }
}
