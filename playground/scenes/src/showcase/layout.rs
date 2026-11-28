//! The shell: header, tab bar, and one component per tab. Nodes name classes; the stylesheet
//! decides how those classes look.

use cappy::Class as ThemeClass;
use cappy::prelude::*;
use cappy_components::DebugPanel;

use super::checklist::ChecklistTab;
use super::counter::CounterTab;
use super::images::images_tab;
use super::motion::MotionTab;
use super::paint::PaintTab;
use super::store::{App, Tab};
use super::style::{Class, class_when};
use super::tiles::TilesTab;

pub fn root() -> Element {
    let app = use_store(App);
    let debug = reactive(false);
    element! {
        div(class = ThemeClass::App) {
            { header(debug) }
            { tab_bar(app) }
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
            show (:app.tab.get() == Tab::Images) {
                { images_tab() }
            }
            { motion_panel(app) }
            if (:debug.get()) {
                DebugPanel()
            }
            text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    }
}

fn tab_bar(app: App) -> Element {
    element! {
        div(class = Class::TabBar) {
            { tab(app, Tab::Counter, "Counter", "tab-counter") }
            { tab(app, Tab::Tiles, "Tiles", "tab-tiles") }
            { tab(app, Tab::Checklist, "Checklist", "tab-checklist") }
            { tab(app, Tab::Paint, "Paint", "tab-paint") }
            { tab(app, Tab::Images, "Images", "tab-images") }
            { motion_tab(app) }
        }
    }
}

fn motion_tab(app: App) -> Element {
    tab(app, Tab::Motion, "Motion", "tab-motion")
}

fn motion_panel(app: App) -> Element {
    element! {
        show (:app.tab.get() == Tab::Motion) {
            MotionTab()
        }
    }
}

fn tab(app: App, tab: Tab, title: &'static str, key: &'static str) -> Element {
    element! {
        div(
            class = ThemeClass::Btn,
            :class = class_when(app.tab.get() == tab, Class::TabActive),
            key = key,
            on_click = move |_| {
                app.select(tab);
                start_motion_loops(tab);
            },
        ) {
            text(title)
        }
    }
}

fn start_motion_loops(tab: Tab) {
    if tab == Tab::Motion {
        Context::current().animate(super::motion::spawn_loops);
    }
}

fn header(debug: Reactive<bool>) -> Element {
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
                    on_click = move |_| debug.update(|on| *on = !*on),
                ) {
                    text(
                        :if debug.get() { "Debug: on" } else { "Debug: off" }.to_owned(),
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
