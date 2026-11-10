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

const EXTRA_TASKS: usize = 16;

#[component]
pub struct ChecklistTab {}

impl Component for ChecklistTab {
    fn render(&self) -> Element {
        let tasks = reactive(
            TASKS
                .iter()
                .enumerate()
                .map(|(index, title)| Task {
                    id: index as u64,
                    title: (*title).to_owned(),
                })
                .chain((0..EXTRA_TASKS).map(|index| {
                    let id = (TASKS.len() + index) as u64;
                    Task {
                        id,
                        title: format!("Scroll row {id}"),
                    }
                }))
                .collect::<Vec<_>>(),
        );
        let next_id = reactive((TASKS.len() + EXTRA_TASKS) as u64);
        element! {
            div(class = [ThemeClass::Card, Class::Grow]) {
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
                { show_row() }
                { branch_row() }
                div(class = Class::ScrollList, key = "tasks") {
                    for (task in :tasks, key = task.id) {
                        { task_row(tasks, reactive(task)) }
                    }
                }
            }
        }
    }
}

/// Hides the counter. The `+1` made before hiding is still there when it comes back.
fn show_row() -> Element {
    let open = reactive(true);
    element! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "show-toggle", on_click = move |_| open.update(|value| *value = !*value)) {
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
    element! {
        div(class = ThemeClass::Row, key = "kept-count") {
            text(:format!("Kept: {}", count), key = "show-count")
            div(class = ThemeClass::Btn, key = "show-increment", on_click = move |_| count.update(|value| *value += 1)) {
                text("+1")
            }
        }
    }
}

/// Switching branches throws the counter away. Coming back starts at zero.
fn branch_row() -> Element {
    let sent = reactive(false);
    element! {
        div(class = ThemeClass::Row) {
            div(class = ThemeClass::Btn, key = "branch-toggle", on_click = move |_| sent.update(|value| *value = !*value)) {
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
    element! {
        div(class = ThemeClass::Row, key = "branch") {
            text(:format!("{name}: {}", count), key = "branch-count")
            div(class = ThemeClass::Btn, key = "branch-increment", on_click = move |_| count.update(|value| *value += 1)) {
                text("+1")
            }
        }
    }
}

fn task_row(tasks: Reactive<Vec<Task>>, task: Reactive<Task>) -> Element {
    let done = reactive(false);
    let id = task.with(|row| row.id);
    element! {
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
