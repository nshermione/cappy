//! The shell: header, tab bar, and one component per tab. Nodes name classes; the stylesheet
//! decides how those classes look.

use std::rc::Rc;
use std::sync::Arc;

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::checklist::ChecklistTab;
use super::counter::CounterTab;
use super::paint::PaintTab;
use super::store::{App, Tab};
use super::style::{Class, class_when};
use super::tiles::TilesTab;

pub fn root(cx: &Context, theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    let app = cx.use_store(App);
    element!((cx) {
        div(class = ThemeClass::App) {
            { header(theme, themes) }
            { tab_bar(app) }
            show (:app.tab.get() == Tab::Counter) {
                @CounterTab()
            }
            show (:app.tab.get() == Tab::Tiles) {
                @TilesTab()
            }
            show (:app.tab.get() == Tab::Checklist) {
                @ChecklistTab()
            }
            show (:app.tab.get() == Tab::Paint) {
                @PaintTab()
            }
            text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    })
}

fn tab_bar(app: App) -> Element {
    element! {
        div(class = Class::TabBar) {
            { tab(app, Tab::Counter, "Counter", "tab-counter") }
            { tab(app, Tab::Tiles, "Tiles", "tab-tiles") }
            { tab(app, Tab::Checklist, "Checklist", "tab-checklist") }
            { tab(app, Tab::Paint, "Paint", "tab-paint") }
        }
    }
}

fn tab(app: App, tab: Tab, title: &'static str, key: &'static str) -> Element {
    element! {
        div(
            class = ThemeClass::Btn,
            :class = class_when(app.tab.get() == tab, Class::TabActive),
            key = key,
            on_click = move |_| app.select(tab),
        ) {
            text(title)
        }
    }
}

fn header(theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    element! {
        div(class = Class::Header) {
            div(class = ThemeClass::Column) {
                text("cappy playground", class = ThemeClass::Title)
                text("Retained UI on wgpu that repaints only what changed", class = ThemeClass::Muted)
            }
            div(
                class = ThemeClass::Btn,
                key = "theme-toggle",
                on_click = move |_| {
                    theme.update(|current| {
                        let index = themes
                            .iter()
                            .position(|candidate| Arc::ptr_eq(candidate, current))
                            .unwrap_or(0);
                        *current = Arc::clone(&themes[(index + 1) % themes.len()]);
                    });
                },
            ) {
                text(:format!("Theme: {}", theme.with(|current| current.name().to_owned())))
            }
        }
    }
}
