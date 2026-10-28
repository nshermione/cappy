//! Structure and behaviour: which nodes exist, what state they read and what clicks do. Every
//! visual decision lives in the stylesheet, so nodes only name their classes.

use std::rc::Rc;
use std::sync::Arc;

use cappy::ClickEvent;
use cappy::prelude::*;

use super::style::Class;

const TILE_COUNT: usize = 6;
const TASKS: [&str; 4] = [
    "Paint only damaged regions",
    "Stay idle when nothing changes",
    "Switch themes without relayout",
    "Host in winit and tao",
];

pub fn root(theme: Signal<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    ui! {
        div(class = Class::App) {
            { header(theme, themes) }
            div(class = Class::CardRow) {
                { counter_card() }
                { tiles_card() }
            }
            { tasks_card() }
            text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = Class::Muted)
        }
    }
}

fn header(theme: Signal<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
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
            div(class = Class::Column) {
                text("cappy playground", class = Class::Title)
                text("Retained UI on wgpu that repaints only what changed", class = Class::Muted)
            }
            div(class = Class::Btn, key = "theme-toggle", on_click = next_theme) {
                text(move || format!("Theme: {}", theme.with(|current| current.name().to_owned())))
            }
        }
    }
}

fn counter_card() -> Element {
    let count = signal(0_i64);
    ui! {
        div(class = Class::Card, class = Class::Grow) {
            text("Counter", class = Class::Subtitle)
            text(move || format!("Count: {}", count.get()), key = "count", class = Class::Count)
            div(class = Class::Row) {
                div(class = Class::Btn, key = "decrement", on_click = move |_| count.update(|value| *value -= 1)) {
                    text("-1")
                }
                div(class = Class::Btn, class = Class::BtnPrimary, key = "increment", on_click = move |_| count.update(|value| *value += 1)) {
                    text("+1")
                }
                div(class = Class::Btn, key = "reset", on_click = move |_| count.set(0)) {
                    text("Reset")
                }
            }
        }
    }
}

fn tiles_card() -> Element {
    let selected = signal(None::<usize>);
    let tiles = (0..TILE_COUNT).map(move |index| tile(index, selected));
    ui! {
        div(class = Class::Card, class = Class::Grow) {
            text("Tiles", class = Class::Subtitle)
            div(class = Class::TileGrid) {
                { ..tiles }
            }
            text(move || match selected.get() {
                Some(index) => format!("Selected tile {}", index + 1),
                None => "Click a tile".to_owned(),
            }, class = Class::Muted, key = "selection")
        }
    }
}

fn tile(index: usize, selected: Signal<Option<usize>>) -> Element {
    ui! {
        div(
            class = Class::Tile,
            class_dyn = move || class_when(selected.get() == Some(index), Class::TileSelected),
            key = format!("tile-{index}"),
            on_click = move |_| selected.set(Some(index)),
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
        div(class = Class::Card) {
            text("Phase 1 checklist", class = Class::Subtitle)
            div(class = Class::Column) {
                { ..rows }
            }
        }
    }
}

fn task_row(index: usize, task: &'static str) -> Element {
    let done = signal(false);
    ui! {
        div(class = Class::Row, key = format!("task-{index}"), on_click = move |_| done.update(|value| *value = !*value)) {
            div(class = Class::Badge, class_dyn = move || class_when(done.get(), Class::BadgeDone)) {
                text(move || if done.get() { "done" } else { "todo" }.to_owned())
            }
            text(task, class_dyn = move || class_when(done.get(), Class::TaskDone))
        }
    }
}

/// The class list for a `class_dyn` binding that adds `class` only while `active`.
fn class_when(active: bool, class: Class) -> String {
    if active {
        class.to_string()
    } else {
        String::new()
    }
}
