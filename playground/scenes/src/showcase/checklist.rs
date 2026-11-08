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
        element!((cx) {
            Container(class = ThemeClass::Card) {
                Container(class = ThemeClass::Row) {
                    Text("Phase 1 checklist", class = ThemeClass::Subtitle)
                    Container(class = ThemeClass::Btn, key = "add-task", on_click = move |_| {
                        let id = next_id.get();
                        next_id.update(|value| *value += 1);
                        tasks.update(|list| {
                            list.push(Task {
                                id,
                                title: format!("Added {id}"),
                            });
                        });
                    }) {
                        Text("Add")
                    }
                }
                Text("show keeps its count while hidden. if starts over.", class = ThemeClass::Muted),
                { show_row(cx) }
                { branch_row(cx) }
                Container(class = ThemeClass::Column, key = "tasks") {
                    for (task in :tasks, key = task.id) {
                        { task_row(cx, tasks, cx.reactive(task)) }
                    }
                }
            }
        })
    }
}

/// Hides the counter. The `+1` made before hiding is still there when it comes back.
fn show_row(cx: &Context) -> Element {
    let open = cx.reactive(true);
    element!((cx) {
        Container(class = ThemeClass::Row) {
            Container(class = ThemeClass::Btn, key = "show-toggle", on_click = move |_| open.update(|value| *value = !*value)) {
                Text(:if open.get() { "Hide kept" } else { "Show kept" }.to_owned())
            }
            show (:open.get()) {
                { kept_count(cx) }
            }
        }
    })
}

fn kept_count(cx: &Context) -> Element {
    let count = cx.reactive(0i64);
    element!((cx) {
        Container(class = ThemeClass::Row, key = "kept-count") {
            Text(:format!("Kept: {}", count), key = "show-count")
            Container(class = ThemeClass::Btn, key = "show-increment", on_click = move |_| count.update(|value| *value += 1)) {
                Text("+1")
            }
        }
    })
}

/// Switching branches throws the counter away. Coming back starts at zero.
fn branch_row(cx: &Context) -> Element {
    let sent = cx.reactive(false);
    element!((cx) {
        Container(class = ThemeClass::Row) {
            Container(class = ThemeClass::Btn, key = "branch-toggle", on_click = move |_| sent.update(|value| *value = !*value)) {
                Text(:if sent.get() { "To draft" } else { "To sent" }.to_owned())
            }
            if (:sent.get()) {
                { branch_count(cx, "Sent") }
            } else {
                { branch_count(cx, "Draft") }
            }
        }
    })
}

fn branch_count(cx: &Context, name: &'static str) -> Element {
    let count = cx.reactive(0i64);
    element!((cx) {
        Container(class = ThemeClass::Row, key = "branch") {
            Text(:format!("{name}: {}", count), key = "branch-count")
            Container(class = ThemeClass::Btn, key = "branch-increment", on_click = move |_| count.update(|value| *value += 1)) {
                Text("+1")
            }
        }
    })
}

fn task_row(cx: &Context, tasks: Reactive<Vec<Task>>, task: Reactive<Task>) -> Element {
    let done = cx.reactive(false);
    let id = task.with(|row| row.id);
    element!((cx) {
        Container(class = ThemeClass::Row, key = format!("task-{id}"), on_click = move |_| done.update(|value| *value = !*value)) {
            Container(class = ThemeClass::Badge, :class = class_when(done.get(), Class::BadgeDone)) {
                Text(:if done.get() { "done" } else { "todo" }.to_owned())
            }
            Text(
                :task.with(|row| row.title.clone()),
                :class = class_when(done.get(), Class::TaskDone),
                key = format!("task-title-{id}"),
            ),
            Container(class = ThemeClass::Btn, key = format!("task-up-{id}"), on_click = move |_| {
                tasks.update(|list| {
                    let Some(index) = list.iter().position(|row| row.id == id) else {
                        return;
                    };
                    if index > 0 {
                        list.swap(index - 1, index);
                    }
                });
            }) {
                Text("Up")
            }
            Container(class = ThemeClass::Btn, key = format!("task-drop-{id}"), on_click = move |_| {
                tasks.update(|list| list.retain(|row| row.id != id));
            }) {
                Text("Drop")
            }
        }
    })
}

#[derive(Clone, PartialEq)]
struct Task {
    id: u64,
    title: String,
}
