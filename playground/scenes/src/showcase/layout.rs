//! Structure and behaviour: which nodes exist, what state they read and what clicks do. Every
//! visual decision lives in the stylesheet, so nodes only name their classes.

use std::rc::Rc;
use std::sync::Arc;

use cappy::Class as ThemeClass;
use cappy::ClickEvent;
use cappy::prelude::*;

use super::badge::Badge;
use super::style::Class;

const TILE_COUNT: usize = 6;
const TASKS: [&str; 4] = [
    "Paint only damaged regions",
    "Stay idle when nothing changes",
    "Switch themes without relayout",
    "Host in winit and tao",
];

pub fn root(theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    ui! {
        div(class = ThemeClass::App) {
            { header(theme, themes) }
            div(class = Class::CardRow) {
                { counter_card() }
                { tiles_card() }
            }
            { tasks_card() }
            text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    }
}

fn header(theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    let theme_name = move || theme.with(|current| current.name().to_owned());
    let next_theme = move |_: &ClickEvent| {
        theme.update(|current| {
            let index = themes
                .iter()
                .position(|candidate| Arc::ptr_eq(candidate, current))
                .unwrap_or(0);
            *current = Arc::clone(&themes[(index + 1) % themes.len()]);
        });
    };
    ui! {
        div(class = Class::Header) {
            div(class = ThemeClass::Column) {
                text("cappy playground", class = ThemeClass::Title)
                text("Retained UI on wgpu that repaints only what changed", class = ThemeClass::Muted)
            }
            { demo_badge() }
            div(class = ThemeClass::Btn, key = "theme-toggle", on_click = next_theme) {
                text(:format!("Theme: {}", theme_name()))
            }
        }
    }
}

#[reactive]
struct CounterState {
    count: i64,
}

#[reactive]
struct AppState {
    counter: CounterState,
    version: i64,
}

fn demo_badge() -> Element {
    let label = reactive(String::from("Badge"));
    let mark_clicked = move |value: &String| label.set(format!("{value}!"));
    ui! {
        @Badge(label = label, on_click = mark_clicked, key = "demo-badge")
    }
}

fn counter_card() -> Element {
    let app_state = AppState::new(CounterState::new(0), 1);
    let decrement = move |_: &ClickEvent| app_state.counter.count.update(|value| *value -= 1);
    let increment = move |_: &ClickEvent| app_state.counter.count.update(|value| *value += 1);
    let reset = move |_: &ClickEvent| app_state.counter.count.set(0);
    ui! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Counter", class = ThemeClass::Subtitle)
            text(:format!("Count: {}", app_state.counter.count), key = "count", class = Class::Count)
            div(class = ThemeClass::Row) {
                div(class = ThemeClass::Btn, key = "decrement", on_click = decrement) {
                    text("-1")
                }
                div(class = [ThemeClass::Btn, ThemeClass::BtnPrimary], key = "increment", on_click = increment) {
                    text("+1")
                }
                div(class = ThemeClass::Btn, key = "reset", on_click = reset) {
                    text("Reset")
                }
            }
        }
    }
}

fn tiles_card() -> Element {
    let selected = reactive(None::<usize>);
    let tiles = (0..TILE_COUNT).map(move |index| tile(index, selected));
    ui! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Tiles", class = ThemeClass::Subtitle)
            div(class = Class::TileGrid) {
                { ..tiles }
            }
            text(:match selected.get() {
                Some(index) => format!("Selected tile {}", index + 1),
                None => "Click a tile".to_owned(),
            }, class = ThemeClass::Muted, key = "selection")
        }
    }
}

fn tile(index: usize, selected: Reactive<Option<usize>>) -> Element {
    let select = move |_: &ClickEvent| selected.set(Some(index));
    ui! {
        div(
            class = ThemeClass::Tile,
            :class = class_when(selected.get() == Some(index), Class::TileSelected),
            key = format!("tile-{index}"),
            on_click = select,
        ) {
            text(format!("Tile {}", index + 1))
        }
    }
}

fn tasks_card() -> Element {
    let rows = TASKS
        .iter()
        .enumerate()
        .map(|(index, task)| task_row(index, task));
    ui! {
        div(class = ThemeClass::Card) {
            text("Phase 1 checklist", class = ThemeClass::Subtitle)
            div(class = ThemeClass::Column) {
                { ..rows }
            }
        }
    }
}

fn task_row(index: usize, task: &'static str) -> Element {
    let done = reactive(false);
    let toggle = move |_: &ClickEvent| done.update(|value| *value = !*value);
    ui! {
        div(class = ThemeClass::Row, key = format!("task-{index}"), on_click = toggle) {
            div(class = ThemeClass::Badge, :class = class_when(done.get(), Class::BadgeDone)) {
                text(:if done.get() { "done" } else { "todo" }.to_owned())
            }
            text(task, :class = class_when(done.get(), Class::TaskDone))
        }
    }
}

/// The class list for a `:class` binding that adds `class` only while `active`.
fn class_when(active: bool, class: Class) -> String {
    if active {
        class.to_string()
    } else {
        String::new()
    }
}
