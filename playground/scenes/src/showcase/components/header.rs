//! Title, debug toggle, and theme switcher. Debug is app-wide state, so this reads the store.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::super::store::AppStore;
use super::super::style::Class;

#[component]
pub(super) struct Header {}

impl Component for Header {
    fn render(&self) -> Element {
        let app = use_store(AppStore);
        let themes = Context::current().themes();
        element! {
            div(class = Class::Header) {
                div(class = ThemeClass::Column) {
                    text("cappy playground", class = ThemeClass::Title)
                    text("Retained UI on wgpu that repaints only what changed", class = ThemeClass::Muted)
                }
                div(class = ThemeClass::Row) {
                    div(
                        class = ThemeClass::Btn,
                        key = "debug-toggle",
                        on_click = move |_| app.debug.update(|on| *on = !*on),
                    ) {
                        text(
                            :if app.debug.get() { "Debug: on" } else { "Debug: off" }.to_owned(),
                            key = "debug-label",
                        )
                    }
                    div(
                        class = ThemeClass::Btn,
                        key = "theme-toggle",
                        on_click = move |_| themes.set(themes.cycle()),
                    ) {
                        text(:format!("Theme: {}", themes.with(|current| current.name().to_owned())))
                    }
                }
            }
        }
    }
}
