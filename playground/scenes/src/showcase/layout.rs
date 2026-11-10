//! The shell: header, tab bar, and one component per tab. Nodes name classes; the stylesheet
//! decides how those classes look.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::checklist::ChecklistTab;
use super::counter::CounterTab;
use super::images::images_tab;
#[cfg(not(feature = "cpu"))]
use super::motion::MotionTab;
use super::paint::PaintTab;
use super::store::{App, Tab};
use super::style::{Class, class_when};
use super::tiles::TilesTab;

pub fn root() -> Element {
    let app = use_store(App);
    element! {
        Container(class = ThemeClass::App) {
            { header() }
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
            Text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    }
}

fn tab_bar(app: App) -> Element {
    element! {
        Container(class = Class::TabBar) {
            { tab(app, Tab::Counter, "Counter", "tab-counter") }
            { tab(app, Tab::Tiles, "Tiles", "tab-tiles") }
            { tab(app, Tab::Checklist, "Checklist", "tab-checklist") }
            { tab(app, Tab::Paint, "Paint", "tab-paint") }
            { tab(app, Tab::Images, "Images", "tab-images") }
            { motion_tab(app) }
        }
    }
}

#[cfg(not(feature = "cpu"))]
fn motion_tab(app: App) -> Element {
    tab(app, Tab::Motion, "Motion", "tab-motion")
}

#[cfg(feature = "cpu")]
fn motion_tab(_app: App) -> Element {
    fragment(std::iter::empty::<Element>())
}

#[cfg(not(feature = "cpu"))]
fn motion_panel(app: App) -> Element {
    element! {
        show (:app.tab.get() == Tab::Motion) {
            MotionTab()
        }
    }
}

#[cfg(feature = "cpu")]
fn motion_panel(_app: App) -> Element {
    fragment(std::iter::empty::<Element>())
}

fn tab(app: App, tab: Tab, title: &'static str, key: &'static str) -> Element {
    let cx = Context::current();
    element! {
        Container(
            class = ThemeClass::Btn,
            :class = class_when(app.tab.get() == tab, Class::TabActive),
            key = key,
            on_click = move |_| {
                app.select(tab);
                start_motion_loops(&cx, tab);
            },
        ) {
            Text(title)
        }
    }
}

#[cfg(not(feature = "cpu"))]
fn start_motion_loops(cx: &Context, tab: Tab) {
    if tab == Tab::Motion {
        cx.animate(super::motion::spawn_loops);
    }
}

#[cfg(feature = "cpu")]
fn start_motion_loops(_cx: &Context, _tab: Tab) {}

fn header() -> Element {
    let themes = Context::current().themes();
    element! {
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
    }
}
