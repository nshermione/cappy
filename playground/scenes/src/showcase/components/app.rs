//! The shell component. It owns the open tab and hosts the header, the tab bar, and one demo tab.

use cappy::Class as ThemeClass;
use cappy::prelude::*;
use cappy_components::DebugPanel;

use super::super::canvas::CanvasTab;
use super::super::checklist::ChecklistTab;
use super::super::counter::CounterTab;
use super::super::images::ImagesTab;
use super::super::motion::MotionTab;
use super::super::paint::PaintTab;
use super::super::store::AppStore;
use super::super::style::Class;
use super::super::tiles::TilesTab;
use super::header::Header;
use super::tab_button::TabButton;

#[derive(Clone, Copy, PartialEq, Eq, Default, Reactive)]
pub(super) enum Tab {
    #[default]
    Counter,
    Tiles,
    Checklist,
    Paint,
    Images,
    Motion,
    Canvas,
}

#[component]
pub struct App {
    tab: Tab,
}

impl Component for App {
    fn render(&self) -> Element {
        let app = use_store(AppStore);
        let selected = self.tab;
        element! {
            div(class = ThemeClass::App) {
                Header()
                div(class = Class::TabBar) {
                    TabButton(selected = selected, tab = Tab::Counter, title = "Counter", key = "tab-counter")
                    TabButton(selected = selected, tab = Tab::Tiles, title = "Tiles", key = "tab-tiles")
                    TabButton(selected = selected, tab = Tab::Checklist, title = "Checklist", key = "tab-checklist")
                    TabButton(selected = selected, tab = Tab::Paint, title = "Paint", key = "tab-paint")
                    TabButton(selected = selected, tab = Tab::Images, title = "Images", key = "tab-images")
                    TabButton(selected = selected, tab = Tab::Motion, title = "Motion", key = "tab-motion")
                    TabButton(selected = selected, tab = Tab::Canvas, title = "Canvas", key = "tab-canvas")
                }
                show (:selected.get() == Tab::Counter) {
                    CounterTab()
                }
                show (:selected.get() == Tab::Tiles) {
                    TilesTab()
                }
                show (:selected.get() == Tab::Checklist) {
                    ChecklistTab()
                }
                show (:selected.get() == Tab::Paint) {
                    PaintTab()
                }
                show (:selected.get() == Tab::Images) {
                    ImagesTab()
                }
                show (:selected.get() == Tab::Motion) {
                    MotionTab()
                }
                show (:selected.get() == Tab::Canvas) {
                    CanvasTab()
                }
                if (:app.debug.get()) {
                    DebugPanel()
                }
                text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
            }
        }
    }
}
