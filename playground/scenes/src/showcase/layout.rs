//! Structure and behaviour: which nodes exist, what state they read and what clicks do. Every
//! visual decision lives in the stylesheet, so nodes only name their classes.

use std::rc::Rc;
use std::sync::Arc;

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::badge;
use super::counter::Counter;
use super::style::Class;

const TILE_COUNT: usize = 6;
const TASKS: [&str; 4] = [
    "Paint only damaged regions",
    "Stay idle when nothing changes",
    "Switch themes without relayout",
    "Host in winit",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Demo {
    Counter,
    Tiles,
    Checklist,
    Badge,
}

impl IntoReactive for Demo {
    type Output = Reactive<Demo>;

    fn into_reactive(self, context: &Context) -> Self::Output {
        Reactive::new(context, self)
    }
}

pub fn root(cx: &Context, theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    let demo = cx.reactive(Demo::Counter);
    let counter_cx = cx.clone();
    let tiles_cx = cx.clone();
    let checklist_cx = cx.clone();
    let badge_cx = cx.clone();
    ui! {
        div(class = ThemeClass::App) {
            { header(theme, themes) }
            { tab_bar(demo) }
            show (:demo.get() == Demo::Counter) {
                { counter_card(&counter_cx) }
            }
            show (:demo.get() == Demo::Tiles) {
                { tiles_card(&tiles_cx) }
            }
            show (:demo.get() == Demo::Checklist) {
                { tasks_card(&checklist_cx) }
            }
            show (:demo.get() == Demo::Badge) {
                { badge::nested(&badge_cx) }
            }
            text("F12 toggles the repaint overlay. Idle frames repaint nothing.", class = ThemeClass::Muted)
        }
    }
}

fn tab_bar(active: Reactive<Demo>) -> Element {
    ui! {
        div(class = Class::TabBar) {
            { tab(active, Demo::Counter, "Counter", "tab-counter") }
            { tab(active, Demo::Tiles, "Tiles", "tab-tiles") }
            { tab(active, Demo::Checklist, "Checklist", "tab-checklist") }
            { tab(active, Demo::Badge, "Badge", "tab-badge") }
        }
    }
}

fn tab(active: Reactive<Demo>, demo: Demo, title: &'static str, key: &'static str) -> Element {
    ui! {
        div(
            class = ThemeClass::Btn,
            :class = class_when(active.get() == demo, Class::TabActive),
            key = key,
            on_click = move |_| active.set(demo),
        ) {
            text(title)
        }
    }
}

fn header(theme: Reactive<Arc<Theme>>, themes: Rc<[Arc<Theme>]>) -> Element {
    ui! {
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

fn counter_card(cx: &Context) -> Element {
    let shared = cx.use_store(Counter);
    let mirror = cx.use_store(Counter);
    let local = Counter::new(cx, 0);
    ui! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Counter", class = ThemeClass::Subtitle)
            text(:format!("Count: {}", shared.count), key = "count", class = Class::Count)
            text(:format!("Count: {}", mirror.count), key = "count-mirror", class = ThemeClass::Muted)
            text(:format!("Local: {}", local.count), key = "count-local", class = ThemeClass::Muted)
            div(class = ThemeClass::Row) {
                div(class = ThemeClass::Btn, key = "decrement", on_click = move |_| shared.decrement()) {
                    text("-1")
                }
                div(class = [ThemeClass::Btn, ThemeClass::BtnPrimary], key = "increment", on_click = move |_| shared.increment()) {
                    text("+1")
                }
                div(class = ThemeClass::Btn, key = "reset", on_click = move |_| shared.reset()) {
                    text("Reset")
                }
                div(class = ThemeClass::Btn, key = "local-increment", on_click = move |_| local.increment()) {
                    text("Local +1")
                }
            }
        }
    }
}

/// Hides the counter. The `+1` made before hiding is still there when it comes back.
fn show_row(cx: &Context) -> Element {
    let open = cx.reactive(true);
    let kept = cx.clone();
    ui! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "show-toggle", on_click = move |_| open.update(|value| *value = !*value)) {
                text(:if open.get() { "Hide kept" } else { "Show kept" }.to_owned())
            }
            show (:open.get()) {
                { kept_count(&kept) }
            }
        }
    }
}

fn kept_count(cx: &Context) -> Element {
    let count = cx.reactive(0i64);
    ui! {
        div(class = ThemeClass::Row, key = "kept-count") {
            text(:format!("Kept: {}", count), key = "show-count")
            div(class = ThemeClass::Btn, key = "show-increment", on_click = move |_| count.update(|value| *value += 1)) {
                text("+1")
            }
        }
    }
}

/// Switching branches throws the counter away. Coming back starts at zero.
fn branch_row(cx: &Context) -> Element {
    let sent = cx.reactive(false);
    let sent_cx = cx.clone();
    let draft_cx = cx.clone();
    ui! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "branch-toggle", on_click = move |_| sent.update(|value| *value = !*value)) {
                text(:if sent.get() { "To draft" } else { "To sent" }.to_owned())
            }
            if (:sent.get()) {
                { branch_count(&sent_cx, "Sent") }
            } else {
                { branch_count(&draft_cx, "Draft") }
            }
        }
    }
}

fn branch_count(cx: &Context, name: &'static str) -> Element {
    let count = cx.reactive(0i64);
    ui! {
        div(class = ThemeClass::Row, key = "branch") {
            text(:format!("{name}: {}", count), key = "branch-count")
            div(class = ThemeClass::Btn, key = "branch-increment", on_click = move |_| count.update(|value| *value += 1)) {
                text("+1")
            }
        }
    }
}

fn tiles_card(cx: &Context) -> Element {
    let selected = cx.reactive(None::<usize>);
    let tiles = (0..TILE_COUNT).map(|index| tile(index, selected));
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
    ui! {
        div(
            class = ThemeClass::Tile,
            :class = class_when(selected.get() == Some(index), Class::TileSelected),
            key = format!("tile-{index}"),
            on_click = move |_| selected.set(Some(index)),
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
    let rows = cx.clone();
    ui! {
        div(class = ThemeClass::Card) {
            div(class = ThemeClass::Row) {
                text("Phase 1 checklist", class = ThemeClass::Subtitle)
                div(class = ThemeClass::Btn, key = "add-task", on_click = move |_| {
                    let id = next_id.get();
                    next_id.update(|value| *value += 1);
                    tasks.update(|list| {
                        list.push(Task {
                            id,
                            title: format!("Added {id}"),
                        });
                    });
                }) {
                    text("Add")
                }
            }
            text("show keeps its count while hidden. if starts over.", class = ThemeClass::Muted),
            { show_row(cx) }
            { branch_row(cx) }
            div(class = ThemeClass::Column, key = "tasks") {
                for (task in :tasks, key = task.id) {
                    { task_row(&rows, tasks, rows.reactive(task)) }
                }
            }
        }
    }
}

fn task_row(cx: &Context, tasks: Reactive<Vec<Task>>, task: Reactive<Task>) -> Element {
    let done = cx.reactive(false);
    let id = task.with(|row| row.id);
    ui! {
        div(class = ThemeClass::Row, key = format!("task-{id}"), on_click = move |_| done.update(|value| *value = !*value)) {
            div(class = ThemeClass::Badge, :class = class_when(done.get(), Class::BadgeDone)) {
                text(:if done.get() { "done" } else { "todo" }.to_owned())
            }
            text(
                :task.with(|row| row.title.clone()),
                :class = class_when(done.get(), Class::TaskDone),
                key = format!("task-title-{id}"),
            ),
            div(class = ThemeClass::Btn, key = format!("task-up-{id}"), on_click = move |_| {
                tasks.update(|list| {
                    let Some(index) = list.iter().position(|row| row.id == id) else {
                        return;
                    };
                    if index > 0 {
                        list.swap(index - 1, index);
                    }
                });
            }) {
                text("Up")
            }
            div(class = ThemeClass::Btn, key = format!("task-drop-{id}"), on_click = move |_| {
                tasks.update(|list| list.retain(|row| row.id != id));
            }) {
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
