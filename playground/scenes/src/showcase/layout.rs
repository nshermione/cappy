//! Structure and behaviour: which nodes exist, what state they read and what clicks do. Every
//! visual decision lives in the stylesheet, so nodes only name their classes.

use std::rc::Rc;
use std::sync::Arc;

use cappy::Class as ThemeClass;
use cappy::ClickEvent;
use cappy::prelude::*;

use super::badge::Badge;
use super::counter::Counter;
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

fn demo_badge() -> Element {
    let label = reactive(String::from("Badge"));
    let mark_clicked = move |value: &String| label.set(format!("{value}!"));
    ui! {
        @Badge(label = label, on_click = mark_clicked, key = "demo-badge") {
            text("in", key = "badge-icon", slot = icon)
            text("out", key = "badge-tail", slot = tail)
        }
    }
}

fn counter_card() -> Element {
    let shared = Counter::singleton();
    let mirror = Counter::singleton();
    let local = Counter::new(0);
    let decrement = move |_: &ClickEvent| shared.decrement();
    let increment = move |_: &ClickEvent| shared.increment();
    let reset = move |_: &ClickEvent| shared.reset();
    let increment_local = move |_: &ClickEvent| local.increment();
    ui! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Counter", class = ThemeClass::Subtitle)
            text(:format!("Count: {}", shared.count), key = "count", class = Class::Count)
            text(:format!("Count: {}", mirror.count), key = "count-mirror", class = ThemeClass::Muted)
            text(:format!("Local: {}", local.count), key = "count-local", class = ThemeClass::Muted)
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
                div(class = ThemeClass::Btn, key = "local-increment", on_click = increment_local) {
                    text("Local +1")
                }
            }
        }
    }
}

/// Hides the counter. The `+1` made before hiding is still there when it comes back.
fn show_row() -> Element {
    let open = reactive(true);
    let toggle = move |_: &ClickEvent| open.update(|value| *value = !*value);
    ui! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "show-toggle", on_click = toggle) {
                text(:if open.get() { "Hide kept" } else { "Show kept" }.to_owned())
            }
            show (:open.get()) {
                { kept_count() }
            }
        }
    }
}

fn kept_count() -> Element {
    let count = reactive(0i64);
    let increment = move |_: &ClickEvent| count.update(|value| *value += 1);
    ui! {
        div(class = ThemeClass::Row, key = "kept-count") {
            text(:format!("Kept: {count}"), key = "show-count")
            div(class = ThemeClass::Btn, key = "show-increment", on_click = increment) {
                text("+1")
            }
        }
    }
}

/// Switching branches throws the counter away. Coming back starts at zero.
fn branch_row() -> Element {
    let sent = reactive(false);
    let toggle = move |_: &ClickEvent| sent.update(|value| *value = !*value);
    ui! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "branch-toggle", on_click = toggle) {
                text(:if sent.get() { "To draft" } else { "To sent" }.to_owned())
            }
            if (:sent.get()) {
                { branch_count("Sent") }
            } else {
                { branch_count("Draft") }
            }
        }
    }
}

fn branch_count(name: &'static str) -> Element {
    let count = reactive(0i64);
    let increment = move |_: &ClickEvent| count.update(|value| *value += 1);
    ui! {
        div(class = ThemeClass::Row, key = "branch") {
            text(:format!("{name}: {count}"), key = "branch-count")
            div(class = ThemeClass::Btn, key = "branch-increment", on_click = increment) {
                text("+1")
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
    let tasks = reactive(
        TASKS
            .iter()
            .enumerate()
            .map(|(index, title)| Task {
                id: index as u64,
                title: (*title).to_owned(),
            })
            .collect::<Vec<_>>(),
    );
    let next_id = reactive(TASKS.len() as u64);
    let add = move |_: &ClickEvent| {
        let id = next_id.get();
        next_id.update(|value| *value += 1);
        tasks.update(|list| {
            list.push(Task {
                id,
                title: format!("Added {id}"),
            });
        });
    };
    ui! {
        div(class = ThemeClass::Card) {
            div(class = ThemeClass::Row) {
                text("Phase 1 checklist", class = ThemeClass::Subtitle)
                div(class = ThemeClass::Btn, key = "add-task", on_click = add) {
                    text("Add")
                }
            }
            text("show keeps its count while hidden. if starts over.", class = ThemeClass::Muted),
            { show_row() }
            { branch_row() }
            div(class = ThemeClass::Column, key = "tasks") {
                for (task in :tasks, key = task.id) {
                    { task_row(tasks, reactive(task)) }
                }
            }
        }
    }
}

fn task_row(tasks: Reactive<Vec<Task>>, task: Reactive<Task>) -> Element {
    let done = reactive(false);
    let id = task.with(|row| row.id);
    let toggle = move |_: &ClickEvent| done.update(|value| *value = !*value);
    let move_up = move |_: &ClickEvent| {
        tasks.update(|list| {
            let Some(index) = list.iter().position(|row| row.id == id) else {
                return;
            };
            if index > 0 {
                list.swap(index - 1, index);
            }
        });
    };
    let remove = move |_: &ClickEvent| {
        tasks.update(|list| list.retain(|row| row.id != id));
    };
    ui! {
        div(class = ThemeClass::Row, key = format!("task-{id}"), on_click = toggle) {
            div(class = ThemeClass::Badge, :class = class_when(done.get(), Class::BadgeDone)) {
                text(:if done.get() { "done" } else { "todo" }.to_owned())
            }
            text(
                :task.with(|row| row.title.clone()),
                :class = class_when(done.get(), Class::TaskDone),
                key = format!("task-title-{id}"),
            ),
            div(class = ThemeClass::Btn, key = format!("task-up-{id}"), on_click = move_up) {
                text("Up")
            }
            div(class = ThemeClass::Btn, key = format!("task-drop-{id}"), on_click = remove) {
                text("Drop")
            }
        }
    }
}

#[derive(Clone, PartialEq)]
struct Task {
    id: u64,
    title: String,
}

/// The class list for a `:class` binding that adds `class` only while `active`.
fn class_when(active: bool, class: Class) -> String {
    if active {
        class.to_string()
    } else {
        String::new()
    }
}
