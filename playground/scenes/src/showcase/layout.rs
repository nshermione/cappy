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
    "Host in winit",
];

pub fn root(cx: &Context, theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    ui! {
        div(class = ThemeClass::App) {
            { header(cx, theme, themes) }
            div(class = Class::CardRow) {
                { counter_card(cx) }
                { tiles_card(cx) }
            }
            { tasks_card(cx) }
            text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    }
}

fn header(cx: &Context, theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    let theme_label = theme.clone();
    let theme_click = theme.clone();
    let theme_name = move || theme_label.with(|current| current.name().to_owned());
    let next_theme = move |_: &ClickEvent| {
        theme_click.update(|current| {
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
            { demo_badge(cx) }
            div(class = ThemeClass::Btn, key = "theme-toggle", on_click = next_theme) {
                text(:format!("Theme: {}", theme_name()))
            }
        }
    }
}

fn demo_badge(cx: &Context) -> Element {
    let label = cx.reactive(String::from("Badge"));
    let label_click = label.clone();
    let mark_clicked = move |value: &String| label_click.set(format!("{value}!"));
    ui! {
        @Badge(label = label, on_click = mark_clicked, key = "demo-badge") {
            text("in", key = "badge-icon", slot = icon)
            text("out", key = "badge-tail", slot = tail)
        }
    }
}

fn counter_card(cx: &Context) -> Element {
    let shared = cx.singleton(Counter);
    let mirror = cx.singleton(Counter);
    let local = Counter::new(cx, 0);
    let decrement_count = shared.clone();
    let increment_count = shared.clone();
    let reset_count = shared.clone();
    let shared_label = shared.clone();
    let mirror_label = mirror.clone();
    let local_label = local.clone();
    let decrement = move |_: &ClickEvent| decrement_count.decrement();
    let increment = move |_: &ClickEvent| increment_count.increment();
    let reset = move |_: &ClickEvent| reset_count.reset();
    let increment_local = move |_: &ClickEvent| local.increment();
    ui! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Counter", class = ThemeClass::Subtitle)
            text(:format!("Count: {}", shared_label.count), key = "count", class = Class::Count)
            text(:format!("Count: {}", mirror_label.count), key = "count-mirror", class = ThemeClass::Muted)
            text(:format!("Local: {}", local_label.count), key = "count-local", class = ThemeClass::Muted)
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
fn show_row(cx: &Context) -> Element {
    let open = cx.reactive(true);
    let open_toggle = open.clone();
    let open_label = open.clone();
    let open_visible = open.clone();
    let toggle = move |_: &ClickEvent| open_toggle.update(|value| *value = !*value);
    let cx_kept = cx.clone();
    ui! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "show-toggle", on_click = toggle) {
                text(:if open_label.get() { "Hide kept" } else { "Show kept" }.to_owned())
            }
            show (:open_visible.get()) {
                { kept_count(&cx_kept) }
            }
        }
    }
}

fn kept_count(cx: &Context) -> Element {
    let count = cx.reactive(0i64);
    let count_label = count.clone();
    let increment = move |_: &ClickEvent| count.update(|value| *value += 1);
    ui! {
        div(class = ThemeClass::Row, key = "kept-count") {
            text(:format!("Kept: {count_label}"), key = "show-count")
            div(class = ThemeClass::Btn, key = "show-increment", on_click = increment) {
                text("+1")
            }
        }
    }
}

/// Switching branches throws the counter away. Coming back starts at zero.
fn branch_row(cx: &Context) -> Element {
    let sent = cx.reactive(false);
    let sent_toggle = sent.clone();
    let sent_label = sent.clone();
    let sent_branch = sent.clone();
    let toggle = move |_: &ClickEvent| sent_toggle.update(|value| *value = !*value);
    let cx_sent = cx.clone();
    let cx_draft = cx.clone();
    ui! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "branch-toggle", on_click = toggle) {
                text(:if sent_label.get() { "To draft" } else { "To sent" }.to_owned())
            }
            if (:sent_branch.get()) {
                { branch_count(&cx_sent, "Sent") }
            } else {
                { branch_count(&cx_draft, "Draft") }
            }
        }
    }
}

fn branch_count(cx: &Context, name: &'static str) -> Element {
    let count = cx.reactive(0i64);
    let count_label = count.clone();
    let increment = move |_: &ClickEvent| count.update(|value| *value += 1);
    ui! {
        div(class = ThemeClass::Row, key = "branch") {
            text(:format!("{name}: {count_label}"), key = "branch-count")
            div(class = ThemeClass::Btn, key = "branch-increment", on_click = increment) {
                text("+1")
            }
        }
    }
}

fn tiles_card(cx: &Context) -> Element {
    let selected = cx.reactive(None::<usize>);
    let selected_caption = selected.clone();
    let tiles = (0..TILE_COUNT).map(move |index| tile(index, selected.clone()));
    ui! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Tiles", class = ThemeClass::Subtitle)
            div(class = Class::TileGrid) {
                { ..tiles }
            }
            text(:match selected_caption.get() {
                Some(index) => format!("Selected tile {}", index + 1),
                None => "Click a tile".to_owned(),
            }, class = ThemeClass::Muted, key = "selection")
        }
    }
}

fn tile(index: usize, selected: Reactive<Option<usize>>) -> Element {
    let selected_class = selected.clone();
    let select = move |_: &ClickEvent| selected.set(Some(index));
    ui! {
        div(
            class = ThemeClass::Tile,
            :class = class_when(selected_class.get() == Some(index), Class::TileSelected),
            key = format!("tile-{index}"),
            on_click = select,
        ) {
            text(format!("Tile {}", index + 1))
        }
    }
}

fn tasks_card(cx: &Context) -> Element {
    let tasks = cx.reactive(
        TASKS
            .iter()
            .enumerate()
            .map(|(index, title)| Task {
                id: index as u64,
                title: (*title).to_owned(),
            })
            .collect::<Vec<_>>(),
    );
    let next_id = cx.reactive(TASKS.len() as u64);
    let tasks_source = tasks.clone();
    let tasks_rows = tasks.clone();
    let cx_rows = cx.clone();
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
            { show_row(cx) }
            { branch_row(cx) }
            div(class = ThemeClass::Column, key = "tasks") {
                for (task in :tasks_source, key = task.id) {
                    { task_row(&cx_rows, tasks_rows.clone(), cx_rows.reactive(task)) }
                }
            }
        }
    }
}

fn task_row(cx: &Context, tasks: Reactive<Vec<Task>>, task: Reactive<Task>) -> Element {
    let done = cx.reactive(false);
    let done_badge = done.clone();
    let done_label = done.clone();
    let done_title = done.clone();
    let id = task.with(|row| row.id);
    let toggle = move |_: &ClickEvent| done.update(|value| *value = !*value);
    let tasks_up = tasks.clone();
    let move_up = move |_: &ClickEvent| {
        tasks_up.update(|list| {
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
            div(class = ThemeClass::Badge, :class = class_when(done_badge.get(), Class::BadgeDone)) {
                text(:if done_label.get() { "done" } else { "todo" }.to_owned())
            }
            text(
                :task.with(|row| row.title.clone()),
                :class = class_when(done_title.get(), Class::TaskDone),
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
