use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::*;
use crate::reactive::{live_signal_count, signal_is_alive, signal_subscriber_count};
use crate::{
    ButtonState, Element, InputEvent, PointerButton, Signal, Slot, div, for_each, fragment,
    if_else, show, signal, slot, text, text_dyn,
};
use cappy_style::{Color, StyleRefinement};

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 800,
    physical_height: 600,
    scale_factor: 1.0,
};

fn counter(count: Signal<i32>) -> Element {
    div().class("app").child(
        div().class("row").key("row").children([
            text_dyn(move || format!("Count: {}", count.get())).key("label"),
            div()
                .class("btn btn-primary")
                .key("button")
                .on_click(move |_| count.update(|value| *value += 1))
                .child(text("+1")),
        ]),
    )
}

fn settled(root: Element) -> Ui {
    let mut ui = Ui::new(root, Theme::light());
    ui.update(VIEWPORT);
    ui.prepare_frame();
    ui
}

fn center_of(ui: &Ui, key: &str) -> Point {
    let rect = ui.node_rect(ui.find_by_key(key).unwrap()).unwrap();
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn regions(outcome: UpdateOutcome) -> Vec<Rect> {
    match outcome {
        UpdateOutcome::Redraw {
            damage: Damage::Regions(regions),
        } => regions,
        other => panic!("expected partial damage, got {other:?}"),
    }
}

fn click(ui: &mut Ui, key: &str) {
    let position = center_of(ui, key);
    ui.handle_event(InputEvent::PointerMoved { position });
    for state in [ButtonState::Pressed, ButtonState::Released] {
        ui.handle_event(InputEvent::PointerButton {
            button: PointerButton::Primary,
            state,
        });
    }
}

#[test]
fn first_update_repaints_everything() {
    let mut ui = Ui::new(counter(signal(0)), Theme::light());
    assert!(ui.needs_update());
    assert_eq!(
        ui.update(VIEWPORT),
        UpdateOutcome::Redraw {
            damage: Damage::Full
        }
    );
    assert_eq!(
        ui.node_text(ui.find_by_key("label").unwrap()),
        Some("Count: 0")
    );
}

#[test]
fn root_fills_the_viewport() {
    let ui = settled(counter(signal(0)));
    assert_eq!(
        ui.node_rect(ui.root()),
        Some(Rect::new(0.0, 0.0, 800.0, 600.0))
    );
}

#[test]
fn nothing_changed_means_idle() {
    let mut ui = settled(counter(signal(0)));
    assert!(!ui.needs_update());
    assert_eq!(ui.update(VIEWPORT), UpdateOutcome::Idle);
    assert_eq!(ui.prepare_frame().unwrap().damage, Damage::None);
}

#[test]
fn signal_change_damages_only_the_bound_text() {
    let count = signal(0);
    let mut ui = settled(counter(count));
    let label = ui.find_by_key("label").unwrap();
    let before = ui.node_rect(label).unwrap();

    count.set(5);
    assert!(ui.needs_update());
    let damaged = regions(ui.update(VIEWPORT));

    assert_eq!(ui.node_text(label), Some("Count: 5"));
    assert_eq!(damaged, vec![before.union(&ui.node_rect(label).unwrap())]);
}

#[test]
fn growing_text_also_damages_the_siblings_it_pushes() {
    let count = signal(0);
    let mut ui = settled(counter(count));
    let button = ui.find_by_key("button").unwrap();
    let button_before = ui.node_rect(button).unwrap();

    count.set(123_456);
    let damaged = regions(ui.update(VIEWPORT));

    let button_after = ui.node_rect(button).unwrap();
    assert_ne!(button_before, button_after);
    let covered = |rect: &Rect| damaged.iter().any(|region| region.contains_rect(rect));
    assert!(covered(&button_before) && covered(&button_after));
}

#[test]
fn clicking_runs_the_handler_and_updates_the_label() {
    let count = signal(0);
    let mut ui = settled(counter(count));
    click(&mut ui, "button");
    assert_eq!(count.get_untracked(), 1);
    ui.update(VIEWPORT);
    assert_eq!(
        ui.node_text(ui.find_by_key("label").unwrap()),
        Some("Count: 1")
    );
}

#[test]
fn click_damage_stays_inside_the_row() {
    let count = signal(0);
    let mut ui = settled(counter(count));
    click(&mut ui, "button");
    let damaged = regions(ui.update(VIEWPORT));
    let row = ui.node_rect(ui.find_by_key("row").unwrap()).unwrap();
    assert!(
        damaged.iter().all(|region| row.contains_rect(region)),
        "{damaged:?} vs {row:?}"
    );
}

#[test]
fn releasing_outside_the_pressed_node_does_not_click() {
    let count = signal(0);
    let mut ui = settled(counter(count));
    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "button"),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(1.0, 1.0),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(count.get_untracked(), 0);
}

#[test]
fn hover_applies_the_hover_variant_and_pointer_cursor() {
    let mut ui = settled(counter(signal(0)));
    let button = ui.find_by_key("button").unwrap();
    let colors = ui.theme().tokens().colors.clone();

    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "button"),
    });
    let damaged = regions(ui.update(VIEWPORT));

    assert_eq!(
        ui.node_style(button).unwrap().visual.background,
        colors.primary_hover
    );
    assert_eq!(ui.cursor(), CursorIcon::Pointer);
    assert_eq!(damaged, vec![ui.node_rect(button).unwrap()]);

    ui.prepare_frame();
    ui.handle_event(InputEvent::PointerLeft);
    ui.update(VIEWPORT);
    assert_eq!(
        ui.node_style(button).unwrap().visual.background,
        colors.primary
    );
    assert_eq!(ui.cursor(), CursorIcon::Default);
}

#[test]
fn hover_handlers_fire_on_enter_and_leave() {
    let hovered = signal(false);
    let root = div().class("app").child(
        div()
            .key("target")
            .style(StyleRefinement::default().size(50))
            .on_hover(move |is_over| hovered.set(is_over)),
    );
    let mut ui = settled(root);
    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "target"),
    });
    assert!(hovered.get_untracked());
    ui.handle_event(InputEvent::PointerLeft);
    assert!(!hovered.get_untracked());
}

#[test]
fn switching_theme_repaints_without_moving_anything() {
    let mut ui = settled(counter(signal(0)));
    let rects: Vec<_> = (0..6)
        .map(|index| ui.node_rect(NodeId::from_index(index)))
        .collect();

    ui.set_theme(Theme::dark());
    assert_eq!(
        ui.update(VIEWPORT),
        UpdateOutcome::Redraw {
            damage: Damage::Full
        }
    );

    let after: Vec<_> = (0..6)
        .map(|index| ui.node_rect(NodeId::from_index(index)))
        .collect();
    assert_eq!(rects, after);
    assert_eq!(
        ui.node_style(ui.root()).unwrap().visual.background,
        Theme::dark().tokens().colors.background
    );
}

#[test]
fn theme_signal_shared_with_the_scene_switches_theme() {
    let theme = signal(Arc::new(Theme::light()));
    let toggle = div()
        .key("toggle")
        .style(StyleRefinement::default().size(40))
        .on_click(move |_| {
            theme.update(|current| {
                *current = Arc::new(if current.name() == "light" {
                    Theme::dark()
                } else {
                    Theme::light()
                });
            });
        });
    let mut ui = Ui::with_theme_signal(div().class("app").child(toggle), theme);
    ui.update(VIEWPORT);
    click(&mut ui, "toggle");
    ui.update(VIEWPORT);
    assert_eq!(ui.theme().name(), "dark");
}

#[test]
fn resizing_relayouts_and_repaints_everything() {
    let mut ui = settled(counter(signal(0)));
    let resized = FrameInput::new(400, 300, 1.0);
    assert_eq!(
        ui.update(resized),
        UpdateOutcome::Redraw {
            damage: Damage::Full
        }
    );
    assert_eq!(
        ui.node_rect(ui.root()),
        Some(Rect::new(0.0, 0.0, 400.0, 300.0))
    );
}

#[test]
fn scale_factor_keeps_logical_layout() {
    let mut ui = settled(counter(signal(0)));
    let label_rect = ui.node_rect(ui.find_by_key("label").unwrap());
    ui.update(FrameInput::new(1600, 1200, 2.0));
    assert_eq!(ui.node_rect(ui.find_by_key("label").unwrap()), label_rect);
    let frame = ui.prepare_frame().unwrap();
    assert_eq!((frame.physical_width, frame.scale_factor), (1600, 2.0));
    assert_eq!(frame.damage, Damage::Full);
}

#[test]
fn waker_fires_when_a_signal_needs_the_ui() {
    let count = signal(0);
    let mut ui = settled(counter(count));
    let wakes = Rc::new(Cell::new(0));
    let counter_handle = wakes.clone();
    ui.set_waker(move || counter_handle.set(counter_handle.get() + 1));
    count.set(1);
    assert_eq!(wakes.get(), 1);
}

#[test]
fn dynamic_classes_and_styles_rebind() {
    let active = signal(false);
    let root = div().class("app").child(
        div()
            .key("chip")
            .class_dyn(move || {
                if active.get() {
                    "btn btn-primary".into()
                } else {
                    "btn".into()
                }
            })
            .style_dyn(move || {
                let width = if active.get() { 120 } else { 80 };
                StyleRefinement::default().width(width)
            }),
    );
    let mut ui = settled(root);
    let chip = ui.find_by_key("chip").unwrap();
    active.set(true);
    ui.update(VIEWPORT);
    let style = ui.node_style(chip).unwrap();
    assert_eq!(style.visual.background, ui.theme().tokens().colors.primary);
    assert_eq!(ui.node_rect(chip).unwrap().width, 120.0);
}

#[test]
fn text_inherits_color_from_its_ancestors() {
    let ui = settled(counter(signal(0)));
    let button = ui.find_by_key("button").unwrap();
    let label = ui.node_children(button)[0];
    assert_eq!(
        ui.node_style(label).unwrap().text.color,
        ui.theme().tokens().colors.on_primary
    );
}

#[test]
fn display_list_paints_back_to_front() {
    let ui = settled(counter(signal(0)));
    let list = ui.display_list();
    let Primitive::Quad(first) = &list[0] else {
        panic!("root background must come first")
    };
    assert_eq!(first.rect, Rect::new(0.0, 0.0, 800.0, 600.0));
    let texts: Vec<&str> = list
        .iter()
        .filter_map(|primitive| match primitive {
            Primitive::Text(text) => Some(&*text.text),
            Primitive::Quad(_) => None,
        })
        .collect();
    assert_eq!(texts, ["Count: 0", "+1"]);
}

#[test]
fn hidden_nodes_are_not_painted_or_hit() {
    let root = div().class("app").child(
        div()
            .key("hidden")
            .style(
                StyleRefinement::default()
                    .display(cappy_style::Display::None)
                    .background(Color::BLACK),
            )
            .child(text("secret")),
    );
    let ui = settled(root);
    assert!(
        ui.display_list()
            .iter()
            .all(|primitive| !matches!(primitive, Primitive::Text(_)))
    );
    assert_eq!(ui.hit_path(Point::new(1.0, 1.0)), vec![ui.root()]);
}

#[test]
fn prepare_frame_before_update_is_none() {
    let mut ui = Ui::new(div(), Theme::light());
    assert!(ui.prepare_frame().is_none());
}

#[test]
fn component_stylesheet_covers_the_rendered_tree() {
    use std::sync::LazyLock;

    use crate::Component;
    use cappy_style::{Color, Stylesheet};

    fn sheet() -> &'static Stylesheet {
        static SHEET: LazyLock<Stylesheet> = LazyLock::new(|| {
            Stylesheet::default()
                .with_class("card", StyleRefinement::default().background(Color::BLACK))
                .with_class(
                    "panel-skin",
                    StyleRefinement::default().background(Color::BLACK),
                )
        });
        &SHEET
    }

    fn own_sheet() -> &'static Stylesheet {
        static SHEET: LazyLock<Stylesheet> = LazyLock::new(|| {
            Stylesheet::default()
                .with_class("card", StyleRefinement::default().background(Color::WHITE))
        });
        &SHEET
    }

    struct Panel;

    impl Component for Panel {
        fn stylesheet() -> &'static Stylesheet {
            sheet()
        }

        fn render(self) -> Element {
            div().class("card").key("panel").children([
                div().class("card").key("inner"),
                div().class("card").key("own").stylesheet(own_sheet()),
            ])
        }
    }

    let ui = settled(Panel.into());
    assert!(!ui.theme().has_class("panel-skin"));
    let panel = ui.node_style(ui.find_by_key("panel").unwrap()).unwrap();
    let inner = ui.node_style(ui.find_by_key("inner").unwrap()).unwrap();
    let own = ui.node_style(ui.find_by_key("own").unwrap()).unwrap();
    assert_eq!(panel.visual.background, Color::BLACK);
    assert_eq!(inner.visual.background, Color::BLACK);
    assert_eq!(own.visual.background, Color::WHITE);
}

#[test]
fn unfilled_slots_are_not_mounted() {
    let tree = div().key("root").children([
        text("a").key("a"),
        slot(Slot::<(), Element>::new(0)),
        text("b").key("b"),
    ]);
    let ui = settled(tree);
    let root = ui.find_by_key("root").unwrap();
    assert_eq!(ui.node_children(root).len(), 2);
    assert_eq!(ui.node_text(ui.find_by_key("a").unwrap()), Some("a"));
    assert_eq!(ui.node_text(ui.find_by_key("b").unwrap()), Some("b"));
}

#[derive(Clone, PartialEq, Debug)]
struct Row {
    id: u64,
    title: String,
}

fn paints(ui: &Ui, text: &str) -> bool {
    ui.display_list().iter().any(|primitive| {
        matches!(primitive, Primitive::Text(painted) if painted.text.as_ref() == text)
    })
}

#[test]
fn show_hides_without_dropping_state() {
    let open = signal(true);
    let label = signal(String::from("one"));
    let state = Rc::new(Cell::new(None));
    let state_slot = Rc::clone(&state);
    let mut ui = settled(div().key("root").children([
        text("head").key("head"),
        show(
            move || open.get(),
            move || {
                let count = signal(1);
                state_slot.set(Some(count));
                text_dyn(move || format!("{}:{}", label.get(), count.get())).key("body")
            },
        ),
        text("tail").key("tail"),
    ]));
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let body = ui.find_by_key("body").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    assert_eq!(ui.node_children(root), &[head, body, tail]);
    assert_eq!(ui.node_text(body), Some("one:1"));
    let signals = live_signal_count();
    let label_subs = signal_subscriber_count(label);
    let open_subs = signal_subscriber_count(open);
    let count = state.get().unwrap();

    open.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("body"), Some(body));
    assert_eq!(ui.node_children(root), &[head, body, tail]);
    let hidden = ui.node_rect(body).unwrap();
    assert_eq!((hidden.width, hidden.height), (0.0, 0.0));
    assert!(!ui.hit_path(Point::new(1.0, 1.0)).contains(&body));
    assert!(!paints(&ui, "one:1"));
    let head_rect = ui.node_rect(head).unwrap();
    let tail_hidden = ui.node_rect(tail).unwrap();
    assert!(tail_hidden.x <= head_rect.x + head_rect.width + 1.0);

    label.set(String::from("two"));
    count.set(5);
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(body), Some("two:5"));
    assert!(!paints(&ui, "two:5"));
    assert_eq!(signal_subscriber_count(label), label_subs);
    assert_eq!(signal_subscriber_count(open), open_subs);
    assert_eq!(live_signal_count(), signals);
    assert!(signal_is_alive(count));

    open.set(true);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("body"), Some(body));
    assert_eq!(ui.node_text(body), Some("two:5"));
    assert!(paints(&ui, "two:5"));
    assert!(ui.node_rect(body).unwrap().width > 0.0);
    let body_rect = ui.node_rect(body).unwrap();
    assert!(ui
        .hit_path(Point::new(
            body_rect.x + body_rect.width / 2.0,
            body_rect.y + body_rect.height / 2.0,
        ))
        .contains(&body));
    assert!(ui.node_rect(tail).unwrap().x > tail_hidden.x + 1.0);
    assert_eq!(signal_subscriber_count(label), label_subs);
    assert_eq!(signal_subscriber_count(open), open_subs);
    assert_eq!(live_signal_count(), signals);
    assert_eq!(count.get(), 5);
}

#[test]
fn if_else_disposes_the_branch_it_leaves() {
    let flag = signal(true);
    let left = signal(1);
    let right = signal(2);
    let created = Rc::new(Cell::new(None));
    let slot = Rc::clone(&created);
    let mut ui = settled(div().key("root").children([
        text("head").key("head"),
        if_else(
            move || flag.get(),
            {
                let slot = Rc::clone(&slot);
                move || {
                    let inner = signal(9);
                    slot.set(Some(inner));
                    text_dyn(move || left.get().to_string()).key("branch")
                }
            },
            move || text_dyn(move || right.get().to_string()).key("branch"),
        ),
        text("tail").key("tail"),
    ]));
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    let first = ui.find_by_key("branch").unwrap();
    assert_eq!(ui.node_children(root), &[head, first, tail]);
    assert_eq!(ui.node_text(first), Some("1"));
    let inner = created.get().unwrap();
    assert_eq!(signal_subscriber_count(left), 1);
    assert_eq!(signal_subscriber_count(right), 0);
    assert_eq!(signal_subscriber_count(flag), 1);
    let while_then = live_signal_count();

    flag.set(false);
    ui.update(VIEWPORT);
    let other = ui.find_by_key("branch").unwrap();
    assert_ne!(other, first);
    assert_eq!(ui.node_children(root), &[head, other, tail]);
    assert_eq!(ui.node_text(other), Some("2"));
    assert!(!signal_is_alive(inner));
    assert_eq!(signal_subscriber_count(left), 0);
    assert_eq!(signal_subscriber_count(right), 1);
    assert_eq!(signal_subscriber_count(flag), 1);
    assert!(live_signal_count() < while_then);

    flag.set(true);
    ui.update(VIEWPORT);
    let again = ui.find_by_key("branch").unwrap();
    assert_ne!(again, first);
    assert_ne!(again, other);
    let restarted = created.get().unwrap();
    assert_ne!(restarted, inner);
    assert!(signal_is_alive(restarted));
    assert_eq!(restarted.get(), 9);
    assert_eq!(signal_subscriber_count(left), 1);
    assert_eq!(signal_subscriber_count(right), 0);

    for _ in 0..4 {
        flag.set(false);
        ui.update(VIEWPORT);
        flag.set(true);
        ui.update(VIEWPORT);
    }
    assert_eq!(signal_subscriber_count(left), 1);
    assert_eq!(signal_subscriber_count(right), 0);
    assert_eq!(signal_subscriber_count(flag), 1);
    assert_eq!(ui.node_children(root).len(), 3);
}

#[test]
fn if_without_else_removes_its_content() {
    let open = signal(true);
    let mut ui = settled(div().key("root").children([
        text("head").key("head"),
        if_else(
            move || open.get(),
            || text("yes").key("yes"),
            || fragment(Vec::<Element>::new()),
        ),
        text("tail").key("tail"),
    ]));
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let yes = ui.find_by_key("yes").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    assert_eq!(ui.node_children(root), &[head, yes, tail]);

    open.set(false);
    ui.update(VIEWPORT);
    assert!(ui.find_by_key("yes").is_none());
    assert_eq!(ui.node_children(root), &[head, tail]);

    open.set(true);
    ui.update(VIEWPORT);
    let fresh = ui.find_by_key("yes").unwrap();
    assert_ne!(fresh, yes);
    assert_eq!(ui.node_children(root), &[head, fresh, tail]);
}

#[test]
fn for_keeps_rows_by_key() {
    let items = signal(vec![
        Row {
            id: 1,
            title: "a".into(),
        },
        Row {
            id: 2,
            title: "b".into(),
        },
    ]);
    let rows = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&rows);
    let mut ui = settled(div().key("root").children([
        text("head").key("head"),
        for_each(
            move |visit| {
                items.with(|list| {
                    for item in list {
                        visit(item);
                    }
                });
            },
            |item| &item.id,
            {
                let captured = Rc::clone(&captured);
                move |item: Signal<Row>| {
                    captured.borrow_mut().push(item);
                    let key = item.with_untracked(|row| row.id.to_string());
                    text_dyn(move || item.with(|row| row.title.clone())).key(key)
                }
            },
        ),
        text("tail").key("tail"),
    ]));
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    let first = ui.find_by_key("1").unwrap();
    let second = ui.find_by_key("2").unwrap();
    assert_eq!(ui.node_children(root), &[head, first, second, tail]);
    let row_signals = rows.borrow().clone();
    assert_eq!(row_signals.len(), 2);
    let signals = live_signal_count();
    assert_eq!(signal_subscriber_count(items), 1);

    items.set(vec![
        Row {
            id: 2,
            title: "b".into(),
        },
        Row {
            id: 1,
            title: "a".into(),
        },
    ]);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("1"), Some(first));
    assert_eq!(ui.find_by_key("2"), Some(second));
    assert_eq!(ui.node_children(root), &[head, second, first, tail]);
    assert_eq!(rows.borrow().len(), 2);
    assert_eq!(live_signal_count(), signals);
    assert!(signal_is_alive(row_signals[0]));
    assert!(signal_is_alive(row_signals[1]));

    items.set(vec![
        Row {
            id: 2,
            title: "b2".into(),
        },
        Row {
            id: 1,
            title: "a".into(),
        },
    ]);
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(second), Some("b2"));
    assert_eq!(ui.node_text(first), Some("a"));
    assert_eq!(row_signals[1].with_untracked(|row| row.title.clone()), "b2");
    assert_eq!(ui.find_by_key("1"), Some(first));
    assert_eq!(live_signal_count(), signals);
    assert_eq!(rows.borrow().len(), 2);

    items.set(vec![Row {
        id: 1,
        title: "a".into(),
    }]);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("1"), Some(first));
    assert!(ui.find_by_key("2").is_none());
    assert!(!signal_is_alive(row_signals[1]));
    assert_eq!(signal_subscriber_count(row_signals[1]), 0);
    assert!(signal_is_alive(row_signals[0]));
    assert_eq!(ui.node_children(root), &[head, first, tail]);
    assert_eq!(signal_subscriber_count(items), 1);

    let after_delete = live_signal_count();
    items.set(vec![Row {
        id: 1,
        title: "a".into(),
    }]);
    ui.update(VIEWPORT);
    assert_eq!(live_signal_count(), after_delete);
    assert_eq!(ui.find_by_key("1"), Some(first));
    assert_eq!(rows.borrow().len(), 2);
}

#[test]
#[should_panic(expected = "duplicate key in for")]
fn duplicate_for_key_panics() {
    let items = signal(vec![1_u64, 1]);
    let _ui = Ui::new(
        for_each(
            move |visit| {
                items.with(|list| {
                    for item in list {
                        visit(item);
                    }
                });
            },
            |item| item,
            |_item| text("x"),
        ),
        Theme::light(),
    );
}

#[test]
fn switching_if_disposes_hidden_show_rows_and_nested_if() {
    let outer = signal(true);
    let panel = signal(true);
    let shown = signal(String::from("keep"));
    let nested = signal(String::from("nest"));
    let items = signal(vec![
        Row {
            id: 1,
            title: "a".into(),
        },
        Row {
            id: 2,
            title: "b".into(),
        },
    ]);
    let rows = Rc::new(RefCell::new(Vec::new()));
    let inner_state = Rc::new(Cell::new(None));
    let mut ui = settled(if_else(
        move || outer.get(),
        {
            let rows = Rc::clone(&rows);
            let inner_state = Rc::clone(&inner_state);
            move || {
                fragment([
                    show(
                        move || panel.get(),
                        {
                            let shown = shown;
                            move || text_dyn(move || shown.get()).key("hidden-show")
                        },
                    ),
                    for_each(
                        move |visit| {
                            items.with(|list| {
                                for item in list {
                                    visit(item);
                                }
                            });
                        },
                        |item| &item.id,
                        {
                            let rows = Rc::clone(&rows);
                            move |item: Signal<Row>| {
                                rows.borrow_mut().push(item);
                                text_dyn(move || item.with(|row| row.title.clone()))
                            }
                        },
                    ),
                    if_else(
                        || true,
                        {
                            let inner_state = Rc::clone(&inner_state);
                            move || {
                                let state = signal(3);
                                inner_state.set(Some(state));
                                text_dyn(move || nested.get()).key("nested")
                            }
                        },
                        || text("no"),
                    ),
                ])
            }
        },
        || text("other").key("other"),
    ));

    let hidden = ui.find_by_key("hidden-show").unwrap();
    assert_eq!(ui.node_text(hidden), Some("keep"));
    assert_eq!(ui.node_text(ui.find_by_key("nested").unwrap()), Some("nest"));
    assert_eq!(signal_subscriber_count(shown), 1);
    assert_eq!(signal_subscriber_count(nested), 1);
    assert_eq!(signal_subscriber_count(items), 1);
    let row_signals = rows.borrow().clone();
    assert_eq!(row_signals.len(), 2);
    let state = inner_state.get().unwrap();
    assert!(row_signals.iter().all(|row| signal_is_alive(*row)));
    assert!(signal_is_alive(state));

    panel.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("hidden-show"), Some(hidden));
    assert_eq!(ui.node_rect(hidden).unwrap().width, 0.0);
    assert_eq!(signal_subscriber_count(shown), 1);
    assert_eq!(signal_subscriber_count(nested), 1);
    assert_eq!(signal_subscriber_count(items), 1);
    assert!(row_signals.iter().all(|row| signal_is_alive(*row)));
    assert!(signal_is_alive(state));
    assert!(ui.find_by_key("nested").is_some());

    outer.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(ui.find_by_key("other").unwrap()), Some("other"));
    assert!(ui.find_by_key("hidden-show").is_none());
    assert!(ui.find_by_key("nested").is_none());
    assert_eq!(signal_subscriber_count(shown), 0);
    assert_eq!(signal_subscriber_count(nested), 0);
    assert_eq!(signal_subscriber_count(items), 0);
    assert_eq!(signal_subscriber_count(panel), 0);
    assert!(row_signals.iter().all(|row| !signal_is_alive(*row)));
    assert!(row_signals.iter().all(|row| signal_subscriber_count(*row) == 0));
    assert!(!signal_is_alive(state));
    assert_eq!(signal_subscriber_count(state), 0);
}

#[test]
fn switching_branches_and_rows_reuses_slots() {
    let flag = signal(true);
    let items = signal(vec![1_u64, 2]);
    let mut ui = settled(div().children([
        if_else(
            move || flag.get(),
            || text_dyn(|| "yes".to_owned()).key("branch"),
            || text_dyn(|| "no".to_owned()).key("branch"),
        ),
        for_each(
            move |visit| {
                items.with(|list| {
                    for item in list {
                        visit(item);
                    }
                });
            },
            |item| item,
            |item| text_dyn(move || item.get().to_string()),
        ),
    ]));
    let nodes = ui.node_slots();
    let bindings = ui.binding_slots();
    let signals = live_signal_count();
    for turn in 0..40 {
        flag.update(|value| *value = !*value);
        if turn % 2 == 0 {
            items.set(vec![1]);
        } else {
            items.set(vec![1, 2]);
        }
        ui.update(VIEWPORT);
    }
    assert_eq!(ui.node_text(ui.find_by_key("branch").unwrap()), Some("yes"));
    assert_eq!(ui.node_slots(), nodes);
    assert_eq!(ui.binding_slots(), bindings);
    assert_eq!(live_signal_count(), signals);
}

#[test]
fn add_and_drop_returns_resources_to_the_baseline() {
    let items = signal(Vec::<u64>::new());
    let mut ui = settled(div().child(for_each(
        move |visit| {
            items.with(|list| {
                for item in list {
                    visit(item);
                }
            });
        },
        |item| item,
        |item| {
            let flag = signal(false);
            let id = item.get_untracked();
            div().key(format!("row-{id}")).children([
                text_dyn(move || item.get().to_string()).key(format!("label-{id}")),
                text_dyn(move || {
                    if flag.get() {
                        "on".to_owned()
                    } else {
                        "off".to_owned()
                    }
                }),
                div()
                    .on_click(move |_| flag.update(|value| *value = !*value))
                    .child(text("Drop")),
            ])
        },
    )));
    let baseline = ui.resources();
    let mut peak_at_thirty = None;
    for round in 0..3 {
        for count in [30_usize, 0, 20, 0, 25, 0, 10, 0] {
            items.set(
                (0..count)
                    .map(|index| round * 1000 + index as u64)
                    .collect(),
            );
            ui.update(VIEWPORT);
            if count == 30 {
                let held = ui.resources();
                if let Some(first) = peak_at_thirty {
                    assert_eq!(held, first);
                } else {
                    peak_at_thirty = Some(held);
                }
            }
        }
    }
    assert_eq!(ui.resources(), baseline);
    assert!(ui.find_by_key("row-0").is_none());
}
