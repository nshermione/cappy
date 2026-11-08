//! The shell: header, tab bar, and one component per tab. Nodes name classes; the stylesheet
//! decides how those classes look.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::checklist::ChecklistTab;
use super::counter::CounterTab;
use super::paint::PaintTab;
use super::store::{App, Tab};
use super::style::{Class, class_when};
use super::tiles::TilesTab;

pub fn root(cx: &Context) -> Element {
    let app = cx.use_store(App);
    element!((cx) {
        Container(class = ThemeClass::App) {
            { header(cx) }
            { tab_bar(cx, app) }
            show (:app.tab.get() == Tab::Counter) {
                CounterTab()
            }
            show (:app.tab.get() == Tab::Tiles) {
                TilesTab()
            }
            show (:app.tab.get() == Tab::Checklist) {
                ChecklistTab()
            }
            show (:app.tab.get() == Tab::Paint) {
                PaintTab()
            }
            Text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    })
}

fn tab_bar(cx: &Context, app: App) -> Element {
    element!((cx) {
        Container(class = Class::TabBar) {
            { tab(cx, app, Tab::Counter, "Counter", "tab-counter") }
            { tab(cx, app, Tab::Tiles, "Tiles", "tab-tiles") }
            { tab(cx, app, Tab::Checklist, "Checklist", "tab-checklist") }
            { tab(cx, app, Tab::Paint, "Paint", "tab-paint") }
        }
    })
}

fn tab(cx: &Context, app: App, tab: Tab, title: &'static str, key: &'static str) -> Element {
    element!((cx) {
        Container(
            class = ThemeClass::Btn,
            :class = class_when(app.tab.get() == tab, Class::TabActive),
            key = key,
            on_click = move |_| app.select(tab),
        ) {
            Text(title)
        }
    })
}

fn header(cx: &Context) -> Element {
    let themes = cx.themes();
    element!((cx) {
        Container(class = Class::Header) {
            Container(class = ThemeClass::Column) {
                Text("cappy playground", class = ThemeClass::Title)
                Text("Retained UI on wgpu that repaints only what changed", class = ThemeClass::Muted)
            }
            Container(
                class = ThemeClass::Btn,
                key = "theme-toggle",
                on_click = move |_| themes.set(themes.cycle()),
            ) {
                Text(:format!("Theme: {}", themes.with(|current| current.name().to_owned())))
            }
        }
    })
}
