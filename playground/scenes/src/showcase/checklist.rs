//! The checklist tab. `show` keeps a count while it is hidden. `if` throws its branch away.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::style::{Class, class_when};

const TASKS: [&str; 4] = [
    "Paint only damaged regions",
    "Stay idle when nothing changes",
    "Switch themes without relayout",
    "Host in winit",
];

#[component]
pub struct ChecklistTab {}

impl Component for ChecklistTab {
    fn render(&self, cx: &Context) -> Element {
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
