use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::{ButtonState, InputEvent, PointerButton, div, signal, text, text_dyn};
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
    let rects: Vec<_> = (0..6).map(|index| ui.node_rect(NodeId(index))).collect();

    ui.set_theme(Theme::dark());
    assert_eq!(
        ui.update(VIEWPORT),
        UpdateOutcome::Redraw {
            damage: Damage::Full
        }
    );

    let after: Vec<_> = (0..6).map(|index| ui.node_rect(NodeId(index))).collect();
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
