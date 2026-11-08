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
        div(class = ThemeClass::App) {
            { header(cx) }
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

fn header(cx: &Context) -> Element {
    let themes = cx.themes();
    element!((cx) {
        div(class = Class::Header) {
            div(class = ThemeClass::Column) {
                text("cappy playground", class = ThemeClass::Title)
                text("Retained UI on wgpu that repaints only what changed", class = ThemeClass::Muted)
            }
            div(
                class = ThemeClass::Btn,
                key = "theme-toggle",
                on_click = move |_| themes.set(themes.cycle()),
            ) {
                text(:format!("Theme: {}", themes.with(|current| current.name().to_owned())))
            }
        }
    })
}
