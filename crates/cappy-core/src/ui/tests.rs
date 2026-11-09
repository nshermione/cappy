use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::*;
use crate::paint::Primitive;
use crate::reactive::{live_signal_count, signal_is_alive, signal_subscriber_count};
use crate::{
    Application, ButtonState, Context, Element, InputEvent, IntoElement, PointerButton, Signal,
    Slot, div, for_each, fragment, if_else, show, slot, text, text_dyn,
};
use cappy_style::{Background, Color, FlexDirection, Overflow, StyleRefinement};

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 800,
    physical_height: 600,
    scale_factor: 1.0,
};

fn context() -> Context {
    Application::new().context().clone()
}

fn counter(count: Signal<i32>) -> Element {
    let label = count.clone();
    div().class("app").child(
        div().class("row").key("row").children([
            text_dyn(move || format!("Count: {}", label.get())).key("label"),
            div()
                .class("btn btn-primary")
                .key("button")
                .on_click(move |_| count.update(|value| *value += 1))
                .child(text("+1")),
        ]),
    )
}

fn settled(context: &Context, root: Element) -> Ui {
    let mut ui = Ui::new(context, root, Theme::light());
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
    let cx = context();
    let mut ui = Ui::new(&cx, counter(cx.signal(0)), Theme::light());
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
    let cx = context();
    let ui = settled(&cx, counter(cx.signal(0)));
    assert_eq!(
        ui.node_rect(ui.root()),
        Some(Rect::new(0.0, 0.0, 800.0, 600.0))
    );
}

#[test]
fn nothing_changed_means_idle() {
    let cx = context();
    let mut ui = settled(&cx, counter(cx.signal(0)));
    assert!(!ui.needs_update());
    assert_eq!(ui.update(VIEWPORT), UpdateOutcome::Idle);
    assert_eq!(ui.prepare_frame().unwrap().damage, Damage::None);
}

#[test]
fn signal_change_damages_only_the_bound_text() {
    let cx = context();
    let count = cx.signal(0);
    let mut ui = settled(&cx, counter(count.clone()));
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
    let cx = context();
    let count = cx.signal(0);
    let mut ui = settled(&cx, counter(count.clone()));
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
    let cx = context();
    let count = cx.signal(0);
    let mut ui = settled(&cx, counter(count.clone()));
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
    let cx = context();
    let count = cx.signal(0);
    let mut ui = settled(&cx, counter(count.clone()));
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
    let cx = context();
    let count = cx.signal(0);
    let mut ui = settled(&cx, counter(count.clone()));
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
    let cx = context();
    let mut ui = settled(&cx, counter(cx.signal(0)));
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
    let cx = context();
    let hovered = cx.signal(false);
    let hovered_flag = hovered.clone();
    let root = div().class("app").child(
        div()
            .key("target")
            .style(StyleRefinement::default().size(50))
            .on_hover(move |is_over| hovered_flag.set(is_over)),
    );
    let mut ui = settled(&cx, root);
    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "target"),
    });
    assert!(hovered.get_untracked());
    ui.handle_event(InputEvent::PointerLeft);
    assert!(!hovered.get_untracked());
}

#[test]
fn switching_theme_repaints_without_moving_anything() {
    let cx = context();
    let mut ui = settled(&cx, counter(cx.signal(0)));
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
    let cx = context();
    cx.set_themes([Theme::light(), Theme::dark()]);
    let theme = cx.current_theme();
    let cx_click = cx.clone();
    let toggle = div()
        .key("toggle")
        .style(StyleRefinement::default().size(40))
        .on_click(move |_| {
            let next = theme.with(|current| {
                if current.name() == "light" {
                    "dark"
                } else {
                    "light"
                }
            });
            cx_click.apply_theme(next);
        });
    let mut ui = Ui::new(&cx, div().class("app").child(toggle), Theme::light());
    ui.update(VIEWPORT);
    click(&mut ui, "toggle");
    ui.update(VIEWPORT);
    assert_eq!(ui.theme().name(), "dark");
}

#[test]
fn resizing_relayouts_and_repaints_everything() {
    let cx = context();
    let mut ui = settled(&cx, counter(cx.signal(0)));
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
    let cx = context();
    let mut ui = settled(&cx, counter(cx.signal(0)));
    let label_rect = ui.node_rect(ui.find_by_key("label").unwrap());
    ui.update(FrameInput::new(1600, 1200, 2.0));
    assert_eq!(ui.node_rect(ui.find_by_key("label").unwrap()), label_rect);
    let frame = ui.prepare_frame().unwrap();
    assert_eq!((frame.physical_width, frame.scale_factor), (1600, 2.0));
    assert_eq!(frame.damage, Damage::Full);
}

#[test]
fn waker_fires_when_a_signal_needs_the_ui() {
    let cx = context();
    let count = cx.signal(0);
    let mut ui = settled(&cx, counter(count.clone()));
    let wakes = Rc::new(Cell::new(0));
    let counter_handle = wakes.clone();
    ui.set_waker(move || counter_handle.set(counter_handle.get() + 1));
    count.set(1);
    assert_eq!(wakes.get(), 1);
}

#[test]
fn dynamic_classes_and_styles_rebind() {
    let cx = context();
    let active = cx.signal(false);
    let active_class = active.clone();
    let active_style = active.clone();
    let root = div().class("app").child(
        div()
            .key("chip")
            .class_dyn(move || {
                if active_class.get() {
                    "btn btn-primary".into()
                } else {
                    "btn".into()
                }
            })
            .style_dyn(move || {
                let width = if active_style.get() { 120 } else { 80 };
                StyleRefinement::default().width(width)
            }),
    );
    let mut ui = settled(&cx, root);
    let chip = ui.find_by_key("chip").unwrap();
    active.set(true);
    ui.update(VIEWPORT);
    let style = ui.node_style(chip).unwrap();
    assert_eq!(style.visual.background, ui.theme().tokens().colors.primary);
    assert_eq!(ui.node_rect(chip).unwrap().width, 120.0);
}

#[test]
fn text_inherits_color_from_its_ancestors() {
    let cx = context();
    let ui = settled(&cx, counter(cx.signal(0)));
    let button = ui.find_by_key("button").unwrap();
    let label = ui.node_children(button)[0];
    assert_eq!(
        ui.node_style(label).unwrap().text.color,
        ui.theme().tokens().colors.on_primary
    );
}

#[test]
fn a_linear_background_is_a_quad_fill() {
    use cappy_style::{Background, linear};

    let cx = context();
    let ui = settled(
        &cx,
        div().style(
            StyleRefinement::default()
                .size(40)
                .background(linear(Color::BLACK, Color::WHITE).to_right()),
        ),
    );
    let Primitive::Quad(quad) = &ui.display_list()[0] else {
        panic!("the root fill must be a quad");
    };
    let Background::Linear {
        start,
        end,
        from,
        to,
    } = quad.background
    else {
        panic!("expected a linear fill");
    };
    assert_eq!((from, to), (Color::BLACK, Color::WHITE));
    assert_eq!(start.x, 0.0);
    assert_eq!(end.x, 1.0);
}

#[test]
fn shadow_repaint_covers_the_old_box_and_the_new_ink() {
    use cappy_style::shadow;

    let cx = context();
    let offset = cx.signal(0.0_f32);
    let offset_for_style = offset.clone();
    let root = div().child(div().key("card").style_dyn(move || {
        StyleRefinement::default()
            .margin(40)
            .size(40)
            .background(Color::WHITE)
            .shadow(shadow(offset_for_style.get(), 0, 0, 0, Color::BLACK))
    }));
    let mut ui = settled(&cx, root);
    let card = ui.node_rect(ui.find_by_key("card").unwrap()).unwrap();
    assert!(
        ui.display_list()
            .iter()
            .any(|primitive| matches!(primitive, Primitive::Shadow(_)))
    );
    offset.set(20.0);
    let damaged = regions(ui.update(VIEWPORT));
    let union = damaged
        .iter()
        .copied()
        .reduce(|left, right| left.union(&right))
        .expect("shadow movement damages something");
    assert!(union.x <= card.x + 0.5);
    assert!(union.right() >= card.right() + 20.0 - 0.5);
}

#[test]
fn a_background_change_without_a_shadow_stays_inside_the_box() {
    let cx = context();
    let dark = cx.signal(false);
    let dark_for_style = dark.clone();
    let root = div().child(div().key("card").style_dyn(move || {
        let color = if dark_for_style.get() {
            Color::BLACK
        } else {
            Color::WHITE
        };
        StyleRefinement::default()
            .margin(40)
            .size(40)
            .background(color)
    }));
    let mut ui = settled(&cx, root);
    let card = ui.node_rect(ui.find_by_key("card").unwrap()).unwrap();
    dark.set(true);
    let damaged = regions(ui.update(VIEWPORT));
    let union = damaged
        .iter()
        .copied()
        .reduce(|left, right| left.union(&right))
        .expect("the recolor damages the card");
    assert!(union.x >= card.x - 0.5);
    assert!(union.right() <= card.right() + 0.5);
    assert!(union.y >= card.y - 0.5);
    assert!(union.bottom() <= card.bottom() + 0.5);
}

#[test]
fn an_image_leaf_paints_its_bitmap() {
    use std::sync::Arc;

    use crate::{ImagePixels, ObjectFit, image};

    let cx = context();
    let pixels = Arc::new(
        ImagePixels::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).expect("2x1 rgba"),
    );
    let ui = settled(
        &cx,
        image(pixels)
            .fit(ObjectFit::Contain)
            .key("photo")
            .style(StyleRefinement::default().size(40)),
    );
    let image = ui
        .display_list()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Image(image) => Some(image),
            _ => None,
        })
        .expect("image primitive");
    assert_eq!(image.fit, ObjectFit::Contain);
    assert!(image.placement().dest.height < image.rect.height);
}

#[test]
fn display_list_paints_back_to_front() {
    let cx = context();
    let ui = settled(&cx, counter(cx.signal(0)));
    let list = ui.display_list();
    let Primitive::Quad(first) = &list[0] else {
        panic!("root background must come first")
    };
    assert_eq!(first.rect, Rect::new(0.0, 0.0, 800.0, 600.0));
    let texts: Vec<&str> = list
        .iter()
        .filter_map(|primitive| match primitive {
            Primitive::Text(text) => Some(&*text.text),
            Primitive::Quad(_) | Primitive::Shadow(_) | Primitive::Image(_) => None,
        })
        .collect();
    assert_eq!(texts, ["Count: 0", "+1"]);
}

#[test]
fn hidden_nodes_are_not_painted_or_hit() {
    let cx = context();
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
    let ui = settled(&cx, root);
    assert!(
        ui.display_list()
            .iter()
            .all(|primitive| !matches!(primitive, Primitive::Text(_)))
    );
    assert_eq!(ui.hit_path(Point::new(1.0, 1.0)), vec![ui.root()]);
}

#[test]
fn prepare_frame_before_update_is_none() {
    let cx = context();
    let mut ui = Ui::new(&cx, div(), Theme::light());
    assert!(ui.prepare_frame().is_none());
}

#[test]
fn component_stylesheet_covers_the_rendered_tree() {
    let cx = context();
    use std::sync::Arc;

    use crate::{Component, HasContext};
    use cappy_style::{Color, Stylesheet};

    fn sheet() -> Arc<Stylesheet> {
        Arc::new(
            Stylesheet::default()
                .with_class("card", StyleRefinement::default().background(Color::BLACK))
                .with_class(
                    "panel-skin",
                    StyleRefinement::default().background(Color::BLACK),
                ),
        )
    }

    fn own_sheet() -> Arc<Stylesheet> {
        Arc::new(
            Stylesheet::default()
                .with_class("card", StyleRefinement::default().background(Color::WHITE)),
        )
    }

    #[derive(Clone)]
    struct Panel {
        context: Context,
    }

    impl HasContext for Panel {
        fn context(&self) -> &Context {
            &self.context
        }
    }

    impl Component for Panel {
        fn stylesheet() -> Option<Arc<Stylesheet>> {
            Some(sheet())
        }

        fn render(&self) -> Element {
            div().class("card").key("panel").children([
                div().class("card").key("inner"),
                div().class("card").key("own").stylesheet(own_sheet()),
            ])
        }
    }

    let ui = settled(
        &cx,
        Panel {
            context: cx.clone(),
        }
        .into_element(),
    );
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
    let cx = context();
    let tree = div().key("root").children([
        text("a").key("a"),
        slot(Slot::<(), Element>::new(0)),
        text("b").key("b"),
    ]);
    let ui = settled(&cx, tree);
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
    ui.display_list().iter().any(
        |primitive| matches!(primitive, Primitive::Text(painted) if painted.text.as_ref() == text),
    )
}

#[test]
fn show_hides_without_dropping_state() {
    let cx = context();
    let open = cx.signal(true);
    let label = cx.signal(String::from("one"));
    let state = Rc::new(RefCell::new(None));
    let state_slot = Rc::clone(&state);
    let open_visible = open.clone();
    let label_text = label.clone();
    let cx_view = cx.clone();
    let mut ui = settled(
        &cx,
        div().key("root").children([
            text("head").key("head"),
            show(
                move || open_visible.get(),
                move || {
                    let count = cx_view.signal(1);
                    state_slot.borrow_mut().replace(count.clone());
                    let label_text = label_text.clone();
                    text_dyn(move || format!("{}:{}", label_text.get(), count.get())).key("body")
                },
            ),
            text("tail").key("tail"),
        ]),
    );
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let body = ui.find_by_key("body").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    assert_eq!(ui.node_children(root), &[head, body, tail]);
    assert_eq!(ui.node_text(body), Some("one:1"));
    let signals = live_signal_count(&cx.runtime);
    let label_subs = signal_subscriber_count(&label);
    let open_subs = signal_subscriber_count(&open);
    let count = state.borrow().clone().unwrap();

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
    assert_eq!(signal_subscriber_count(&label), label_subs);
    assert_eq!(signal_subscriber_count(&open), open_subs);
    assert_eq!(live_signal_count(&cx.runtime), signals);
    assert!(signal_is_alive(&count));

    open.set(true);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("body"), Some(body));
    assert_eq!(ui.node_text(body), Some("two:5"));
    assert!(paints(&ui, "two:5"));
    assert!(ui.node_rect(body).unwrap().width > 0.0);
    let body_rect = ui.node_rect(body).unwrap();
    assert!(
        ui.hit_path(Point::new(
            body_rect.x + body_rect.width / 2.0,
            body_rect.y + body_rect.height / 2.0,
        ))
        .contains(&body)
    );
    assert!(ui.node_rect(tail).unwrap().x > tail_hidden.x + 1.0);
    assert_eq!(signal_subscriber_count(&label), label_subs);
    assert_eq!(signal_subscriber_count(&open), open_subs);
    assert_eq!(live_signal_count(&cx.runtime), signals);
    assert_eq!(count.get(), 5);
}

#[test]
fn if_else_disposes_the_branch_it_leaves() {
    let cx = context();
    let flag = cx.signal(true);
    let left = cx.signal(1);
    let right = cx.signal(2);
    let created = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&created);
    let flag_branch = flag.clone();
    let left_text = left.clone();
    let right_text = right.clone();
    let cx_view = cx.clone();
    let mut ui = settled(
        &cx,
        div().key("root").children([
            text("head").key("head"),
            if_else(
                move || flag_branch.get(),
                {
                    let slot = Rc::clone(&slot);
                    move || {
                        let inner = cx_view.signal(9);
                        slot.borrow_mut().replace(inner);
                        let left_text = left_text.clone();
                        text_dyn(move || left_text.get().to_string()).key("branch")
                    }
                },
                move || {
                    let right_text = right_text.clone();
                    text_dyn(move || right_text.get().to_string()).key("branch")
                },
            ),
            text("tail").key("tail"),
        ]),
    );
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    let first = ui.find_by_key("branch").unwrap();
    assert_eq!(ui.node_children(root), &[head, first, tail]);
    assert_eq!(ui.node_text(first), Some("1"));
    let inner = created.borrow().clone().unwrap();
    assert_eq!(signal_subscriber_count(&left), 1);
    assert_eq!(signal_subscriber_count(&right), 0);
    assert_eq!(signal_subscriber_count(&flag), 1);
    let while_then = live_signal_count(&cx.runtime);

    flag.set(false);
    ui.update(VIEWPORT);
    let other = ui.find_by_key("branch").unwrap();
    assert_ne!(other, first);
    assert_eq!(ui.node_children(root), &[head, other, tail]);
    assert_eq!(ui.node_text(other), Some("2"));
    assert!(!signal_is_alive(&inner));
    assert_eq!(signal_subscriber_count(&left), 0);
    assert_eq!(signal_subscriber_count(&right), 1);
    assert_eq!(signal_subscriber_count(&flag), 1);
    assert!(live_signal_count(&cx.runtime) < while_then);

    flag.set(true);
    ui.update(VIEWPORT);
    let again = ui.find_by_key("branch").unwrap();
    assert_ne!(again, first);
    assert_ne!(again, other);
    let restarted = created.borrow().clone().unwrap();
    assert_ne!(restarted, inner);
    assert!(signal_is_alive(&restarted));
    assert_eq!(restarted.get(), 9);
    assert_eq!(signal_subscriber_count(&left), 1);
    assert_eq!(signal_subscriber_count(&right), 0);

    for _ in 0..4 {
        flag.set(false);
        ui.update(VIEWPORT);
        flag.set(true);
        ui.update(VIEWPORT);
    }
    assert_eq!(signal_subscriber_count(&left), 1);
    assert_eq!(signal_subscriber_count(&right), 0);
    assert_eq!(signal_subscriber_count(&flag), 1);
    assert_eq!(ui.node_children(root).len(), 3);
}

#[test]
fn if_without_else_removes_its_content() {
    let cx = context();
    let open = cx.signal(true);
    let open_visible = open.clone();
    let mut ui = settled(
        &cx,
        div().key("root").children([
            text("head").key("head"),
            if_else(
                move || open_visible.get(),
                || text("yes").key("yes"),
                || fragment(Vec::<Element>::new()),
            ),
            text("tail").key("tail"),
        ]),
    );
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
    let cx = context();
    let items = cx.signal(vec![
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
    let items_source = items.clone();
    let mut ui = settled(
        &cx,
        div().key("root").children([
            text("head").key("head"),
            for_each(
                move |visit| {
                    items_source.with(|list| {
                        for item in list {
                            visit(item);
                        }
                    });
                },
                |item| &item.id,
                {
                    let captured = Rc::clone(&captured);
                    move |item: Signal<Row>| {
                        captured.borrow_mut().push(item.clone());
                        let key = item.with_untracked(|row| row.id.to_string());
                        text_dyn(move || item.with(|row| row.title.clone())).key(key)
                    }
                },
            ),
            text("tail").key("tail"),
        ]),
    );
    let root = ui.find_by_key("root").unwrap();
    let head = ui.find_by_key("head").unwrap();
    let tail = ui.find_by_key("tail").unwrap();
    let first = ui.find_by_key("1").unwrap();
    let second = ui.find_by_key("2").unwrap();
    assert_eq!(ui.node_children(root), &[head, first, second, tail]);
    let row_signals = rows.borrow().clone();
    assert_eq!(row_signals.len(), 2);
    let signals = live_signal_count(&cx.runtime);
    assert_eq!(signal_subscriber_count(&items), 1);

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
    assert_eq!(live_signal_count(&cx.runtime), signals);
    assert!(signal_is_alive(&row_signals[0]));
    assert!(signal_is_alive(&row_signals[1]));

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
    assert_eq!(live_signal_count(&cx.runtime), signals);
    assert_eq!(rows.borrow().len(), 2);

    items.set(vec![Row {
        id: 1,
        title: "a".into(),
    }]);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("1"), Some(first));
    assert!(ui.find_by_key("2").is_none());
    assert!(!signal_is_alive(&row_signals[1]));
    assert_eq!(signal_subscriber_count(&row_signals[1]), 0);
    assert!(signal_is_alive(&row_signals[0]));
    assert_eq!(ui.node_children(root), &[head, first, tail]);
    assert_eq!(signal_subscriber_count(&items), 1);

    let after_delete = live_signal_count(&cx.runtime);
    items.set(vec![Row {
        id: 1,
        title: "a".into(),
    }]);
    ui.update(VIEWPORT);
    assert_eq!(live_signal_count(&cx.runtime), after_delete);
    assert_eq!(ui.find_by_key("1"), Some(first));
    assert_eq!(rows.borrow().len(), 2);
}

#[test]
#[should_panic(expected = "duplicate key in for")]
fn duplicate_for_key_panics() {
    let cx = context();
    let items = cx.signal(vec![1_u64, 1]);
    let _ui = Ui::new(
        &cx,
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
    let cx = context();
    let outer = cx.signal(true);
    let panel = cx.signal(true);
    let shown = cx.signal(String::from("keep"));
    let nested = cx.signal(String::from("nest"));
    let items = cx.signal(vec![
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
    let inner_state = Rc::new(RefCell::new(None));
    let outer_branch = outer.clone();
    let panel_open = panel.clone();
    let shown_text = shown.clone();
    let nested_text = nested.clone();
    let items_source = items.clone();
    let cx_view = cx.clone();
    let mut ui = settled(
        &cx,
        if_else(
            move || outer_branch.get(),
            {
                let rows = Rc::clone(&rows);
                let inner_state = Rc::clone(&inner_state);
                move || {
                    let cx_view = cx_view.clone();
                    fragment([
                        show(
                            {
                                let panel_open = panel_open.clone();
                                move || panel_open.get()
                            },
                            {
                                let shown_text = shown_text.clone();
                                move || {
                                    let shown_text = shown_text.clone();
                                    text_dyn(move || shown_text.get()).key("hidden-show")
                                }
                            },
                        ),
                        for_each(
                            {
                                let items_source = items_source.clone();
                                move |visit| {
                                    items_source.with(|list| {
                                        for item in list {
                                            visit(item);
                                        }
                                    });
                                }
                            },
                            |item| &item.id,
                            {
                                let rows = Rc::clone(&rows);
                                move |item: Signal<Row>| {
                                    rows.borrow_mut().push(item.clone());
                                    text_dyn(move || item.with(|row| row.title.clone()))
                                }
                            },
                        ),
                        if_else(
                            || true,
                            {
                                let inner_state = Rc::clone(&inner_state);
                                let nested_text = nested_text.clone();
                                move || {
                                    let state = cx_view.signal(3);
                                    inner_state.borrow_mut().replace(state);
                                    let nested_text = nested_text.clone();
                                    text_dyn(move || nested_text.get()).key("nested")
                                }
                            },
                            || text("no"),
                        ),
                    ])
                }
            },
            || text("other").key("other"),
        ),
    );

    let hidden = ui.find_by_key("hidden-show").unwrap();
    assert_eq!(ui.node_text(hidden), Some("keep"));
    assert_eq!(
        ui.node_text(ui.find_by_key("nested").unwrap()),
        Some("nest")
    );
    assert_eq!(signal_subscriber_count(&shown), 1);
    assert_eq!(signal_subscriber_count(&nested), 1);
    assert_eq!(signal_subscriber_count(&items), 1);
    let row_signals = rows.borrow().clone();
    assert_eq!(row_signals.len(), 2);
    let state = inner_state.borrow().clone().unwrap();
    assert!(row_signals.iter().all(|row| signal_is_alive(&*row)));
    assert!(signal_is_alive(&state));

    panel.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ui.find_by_key("hidden-show"), Some(hidden));
    assert_eq!(ui.node_rect(hidden).unwrap().width, 0.0);
    assert_eq!(signal_subscriber_count(&shown), 1);
    assert_eq!(signal_subscriber_count(&nested), 1);
    assert_eq!(signal_subscriber_count(&items), 1);
    assert!(row_signals.iter().all(|row| signal_is_alive(&*row)));
    assert!(signal_is_alive(&state));
    assert!(ui.find_by_key("nested").is_some());

    outer.set(false);
    ui.update(VIEWPORT);
    assert_eq!(
        ui.node_text(ui.find_by_key("other").unwrap()),
        Some("other")
    );
    assert!(ui.find_by_key("hidden-show").is_none());
    assert!(ui.find_by_key("nested").is_none());
    assert_eq!(signal_subscriber_count(&shown), 0);
    assert_eq!(signal_subscriber_count(&nested), 0);
    assert_eq!(signal_subscriber_count(&items), 0);
    assert_eq!(signal_subscriber_count(&panel), 0);
    assert!(row_signals.iter().all(|row| !signal_is_alive(&*row)));
    assert!(
        row_signals
            .iter()
            .all(|row| signal_subscriber_count(&*row) == 0)
    );
    assert!(!signal_is_alive(&state));
    assert_eq!(signal_subscriber_count(&state), 0);
}

#[test]
fn switching_branches_and_rows_reuses_slots() {
    let cx = context();
    let flag = cx.signal(true);
    let items = cx.signal(vec![1_u64, 2]);
    let flag_branch = flag.clone();
    let items_source = items.clone();
    let mut ui = settled(
        &cx,
        div().children([
            if_else(
                move || flag_branch.get(),
                || text_dyn(|| "yes".to_owned()).key("branch"),
                || text_dyn(|| "no".to_owned()).key("branch"),
            ),
            for_each(
                move |visit| {
                    items_source.with(|list| {
                        for item in list {
                            visit(item);
                        }
                    });
                },
                |item| item,
                |item| text_dyn(move || item.get().to_string()),
            ),
        ]),
    );
    let nodes = ui.node_slots();
    let bindings = ui.binding_slots();
    let signals = live_signal_count(&cx.runtime);
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
    assert_eq!(live_signal_count(&cx.runtime), signals);
}

#[test]
fn add_and_drop_returns_resources_to_the_baseline() {
    let cx = context();
    let items = cx.signal(Vec::<u64>::new());
    let items_source = items.clone();
    let cx_row = cx.clone();
    let mut ui = settled(
        &cx,
        div().child(for_each(
            move |visit| {
                items_source.with(|list| {
                    for item in list {
                        visit(item);
                    }
                });
            },
            |item| item,
            move |item| {
                let flag = cx_row.signal(false);
                let shown = flag.clone();
                let id = item.get_untracked();
                div().key(format!("row-{id}")).children([
                    text_dyn(move || item.get().to_string()).key(format!("label-{id}")),
                    text_dyn(move || {
                        if shown.get() {
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
        )),
    );
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

fn scroll_column(overflow: Overflow) -> Element {
    div().child(
        div()
            .key("scroller")
            .style(
                StyleRefinement::default()
                    .direction(FlexDirection::Column)
                    .width(120)
                    .height(80)
                    .overflow_y(overflow),
            )
            .child(
                div().key("tall").style(
                    StyleRefinement::default()
                        .width(100)
                        .height(200)
                        .flex_shrink(0.0)
                        .background(Color::BLACK),
                ),
            ),
    )
}

fn settle_scroll(ui: &mut Ui, start: Instant) {
    let mut now = start;
    for _ in 0..40 {
        now += Duration::from_millis(16);
        ui.update_at(VIEWPORT, now);
        if !ui.needs_update() {
            return;
        }
    }
    panic!("scroll ease did not reach its target");
}

#[test]
fn overflow_auto_clips_layout_eases_to_the_wheel_target_and_stops() {
    let cx = context();
    let start = Instant::now();
    let mut ui = Ui::new(&cx, scroll_column(Overflow::Auto), Theme::light());
    ui.update_at(VIEWPORT, start);
    ui.prepare_frame();
    let scroller = ui.node_rect(ui.find_by_key("scroller").unwrap()).unwrap();
    let tall_id = ui.find_by_key("tall").unwrap();
    let before = ui.node_rect(tall_id).unwrap();
    assert_eq!(scroller.height, 80.0);
    assert_eq!(before.height, 200.0);

    let clip = ui
        .display_list()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Quad(quad) if quad.rect == before => quad.clip,
            _ => None,
        });
    let clip = clip.expect("the tall box paints a quad");
    assert_eq!(clip.y, scroller.y);
    assert_eq!(clip.height, scroller.height);
    assert!(
        ui.display_list().iter().any(|primitive| {
            matches!(
                primitive,
                Primitive::Quad(quad)
                    if quad.rect.width == 4.0 && scroller.contains_rect(&quad.rect)
            )
        }),
        "an overflowing auto box paints a thumb inside the scrollport"
    );

    let below = Point::new(scroller.x + 8.0, scroller.bottom() + 10.0);
    assert!(!ui.hit_path(below).contains(&tall_id));
    let inside = Point::new(scroller.x + 8.0, scroller.y + 8.0);
    assert!(ui.hit_path(inside).contains(&tall_id));

    ui.handle_event(InputEvent::PointerMoved { position: inside });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -40.0),
    });
    ui.update_at(VIEWPORT, start);
    assert_eq!(ui.node_rect(tall_id).unwrap().y, before.y);
    assert!(ui.needs_update());

    let stepped = ui.update_at(VIEWPORT, start + Duration::from_millis(32));
    let moved = before.y - ui.node_rect(tall_id).unwrap().y;
    assert!(moved > 1.0 && moved < 40.0, "eased {moved} px");
    match stepped {
        UpdateOutcome::Redraw {
            damage: Damage::Regions(regions),
        } => {
            assert!(!regions.is_empty());
            for region in regions {
                assert_eq!(region.intersection(&scroller), Some(region));
            }
        }
        other => panic!("expected clipped damage, got {other:?}"),
    }

    settle_scroll(&mut ui, start + Duration::from_millis(32));
    assert!(!ui.needs_update());
    assert!((ui.node_rect(tall_id).unwrap().y - (before.y - 40.0)).abs() < 0.5);
}

#[test]
fn overflow_hidden_clips_without_scrolling() {
    let cx = context();
    let start = Instant::now();
    let mut ui = Ui::new(&cx, scroll_column(Overflow::Hidden), Theme::light());
    ui.update_at(VIEWPORT, start);
    let scroller = ui.node_rect(ui.find_by_key("scroller").unwrap()).unwrap();
    let tall_id = ui.find_by_key("tall").unwrap();
    let before = ui.node_rect(tall_id).unwrap().y;
    let inside = Point::new(scroller.x + 8.0, scroller.y + 8.0);
    ui.handle_event(InputEvent::PointerMoved { position: inside });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -40.0),
    });
    ui.update_at(VIEWPORT, start + Duration::from_millis(32));
    assert_eq!(ui.node_rect(tall_id).unwrap().y, before);
    assert!(!ui.needs_update());
    let below = Point::new(scroller.x + 8.0, scroller.bottom() + 10.0);
    assert!(!ui.hit_path(below).contains(&tall_id));
}

#[test]
fn nested_scroll_gives_leftover_wheel_to_the_parent() {
    let cx = context();
    let root = div().child(
        div()
            .key("outer")
            .style(
                StyleRefinement::default()
                    .direction(FlexDirection::Column)
                    .width(140)
                    .height(120)
                    .overflow_y(Overflow::Auto),
            )
            .children([
                div()
                    .key("inner")
                    .style(
                        StyleRefinement::default()
                            .direction(FlexDirection::Column)
                            .width(120)
                            .height(80)
                            .flex_shrink(0.0)
                            .overflow_y(Overflow::Auto),
                    )
                    .child(
                        div().key("inner-body").style(
                            StyleRefinement::default()
                                .height(200)
                                .flex_shrink(0.0)
                                .background(Color::BLACK),
                        ),
                    ),
                div().key("outer-body").style(
                    StyleRefinement::default()
                        .height(200)
                        .flex_shrink(0.0)
                        .background(Color::BLACK),
                ),
            ]),
    );
    let start = Instant::now();
    let mut ui = Ui::new(&cx, root, Theme::light());
    ui.update_at(VIEWPORT, start);
    let inner = ui.node_rect(ui.find_by_key("inner").unwrap()).unwrap();
    let inner_body = ui
        .node_rect(ui.find_by_key("inner-body").unwrap())
        .unwrap()
        .y;
    let outer_body = ui
        .node_rect(ui.find_by_key("outer-body").unwrap())
        .unwrap()
        .y;
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(inner.x + 4.0, inner.y + 4.0),
    });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -1000.0),
    });
    settle_scroll(&mut ui, start);
    assert!(!ui.needs_update());
    let inner_after = ui.node_rect(ui.find_by_key("inner").unwrap()).unwrap().y;
    let inner_body_after = ui
        .node_rect(ui.find_by_key("inner-body").unwrap())
        .unwrap()
        .y;
    let outer_body_after = ui
        .node_rect(ui.find_by_key("outer-body").unwrap())
        .unwrap()
        .y;
    assert!((inner.y - inner_after - 160.0).abs() < 0.5);
    assert!((inner_body - inner_body_after - 280.0).abs() < 0.5);
    assert!((outer_body - outer_body_after - 160.0).abs() < 0.5);
}

#[test]
fn dragging_the_thumb_scrolls_immediately_and_does_not_click() {
    let cx = context();
    let clicks = cx.signal(0i32);
    let clicked = clicks.clone();
    let root = div().child(
        div()
            .key("scroller")
            .style(
                StyleRefinement::default()
                    .direction(FlexDirection::Column)
                    .width(120)
                    .height(80)
                    .overflow_y(Overflow::Auto),
            )
            .child(
                div()
                    .key("tall")
                    .style(
                        StyleRefinement::default()
                            .width(120)
                            .height(200)
                            .flex_shrink(0.0)
                            .background(Color::BLACK),
                    )
                    .on_click(move |_| clicked.update(|value| *value += 1)),
            ),
    );
    let start = Instant::now();
    let mut ui = Ui::new(&cx, root, Theme::light());
    ui.update_at(VIEWPORT, start);
    ui.prepare_frame();
    let scroller = ui.node_rect(ui.find_by_key("scroller").unwrap()).unwrap();
    let tall = ui.find_by_key("tall").unwrap();
    let before = ui.node_rect(tall).unwrap().y;
    let thumb = ui
        .display_list()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Quad(quad)
                if quad.rect.width == 4.0 && scroller.contains_rect(&quad.rect) =>
            {
                Some(quad.rect)
            }
            _ => None,
        })
        .expect("thumb");
    let on_thumb = Point::new(thumb.x + thumb.width * 0.5, thumb.y + thumb.height * 0.5);

    ui.handle_event(InputEvent::PointerMoved { position: on_thumb });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(on_thumb.x, on_thumb.y + 20.0),
    });
    ui.update_at(VIEWPORT, start);
    let dragged = before - ui.node_rect(tall).unwrap().y;
    assert!(dragged > 1.0, "dragged {dragged}");
    assert!(
        !ui.needs_update(),
        "a drag writes the offset, it does not ease"
    );

    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(clicks.get_untracked(), 0);

    let on_track = Point::new(scroller.right() - 4.0, scroller.bottom() - 4.0);
    ui.handle_event(InputEvent::PointerMoved { position: on_track });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.update_at(VIEWPORT, start);
    assert!(ui.node_rect(tall).unwrap().y < before - dragged);
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(on_track.x, scroller.bottom() + 80.0),
    });
    ui.update_at(VIEWPORT, start);
    assert!((ui.node_rect(tall).unwrap().y - (before - 120.0)).abs() < 0.5);
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(clicks.get_untracked(), 0);

    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(scroller.x + 8.0, scroller.y + 8.0),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(clicks.get_untracked(), 1);
}

fn sized_box(key: &str, color: Color) -> Element {
    div().key(key).style(
        StyleRefinement::default()
            .width(40)
            .height(20)
            .background(color),
    )
}

fn solid_fill(ui: &Ui, key: &str) -> Color {
    let rect = ui.node_rect(ui.find_by_key(key).unwrap()).unwrap();
    ui.display_list()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Quad(quad) if quad.rect == rect => match quad.background {
                Background::Solid(color) => Some(color),
                Background::Linear { .. } => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("{key} has no solid fill"))
}

#[test]
fn translation_moves_the_node_without_moving_its_sibling() {
    let cx = context();
    let mut ui = settled(
        &cx,
        div().children([sized_box("a", Color::BLACK), sized_box("b", Color::BLACK)]),
    );
    let a = ui.find_by_key("a").unwrap();
    let b = ui.find_by_key("b").unwrap();
    let a_start = ui.node_rect(a).unwrap();
    let b_start = ui.node_rect(b).unwrap();
    let start = Instant::now();
    ui.animate(
        a,
        Motion::translation(0.0, 40.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    assert_eq!(ui.node_rect(a).unwrap(), a_start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    let moved = ui.node_rect(a).unwrap();
    assert!((moved.y - (a_start.y + 20.0)).abs() < 0.01);
    assert_eq!(ui.node_rect(b).unwrap(), b_start);
    let hit = Point::new(moved.x + 1.0, moved.y + 1.0);
    assert!(ui.hit_path(hit).contains(&a));
    let old = Point::new(a_start.x + 1.0, a_start.y + 1.0);
    assert!(!ui.hit_path(old).contains(&a));
}

#[test]
fn a_child_moves_with_its_parent_translation() {
    let cx = context();
    let mut ui = settled(
        &cx,
        div().key("parent").child(sized_box("child", Color::BLACK)),
    );
    let parent = ui.find_by_key("parent").unwrap();
    let child = ui.find_by_key("child").unwrap();
    let child_start = ui.node_rect(child).unwrap();
    let start = Instant::now();
    ui.animate(
        parent,
        Motion::translation(0.0, 12.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    let child_now = ui.node_rect(child).unwrap();
    assert!((child_now.y - (child_start.y + 6.0)).abs() < 0.01);
}

#[test]
fn opacity_scales_the_fill_and_a_new_animate_replaces_the_tween() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("box", Color::rgb(0, 0, 255)));
    let id = ui.find_by_key("box").unwrap();
    let start = Instant::now();
    ui.animate(
        id,
        Motion::opacity(0.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    let faded = solid_fill(&ui, "box");
    assert!((faded.a - 0.5).abs() < 0.01);
    assert_eq!((faded.r, faded.g, faded.b), (0.0, 0.0, 1.0));

    ui.animate(
        id,
        Motion::opacity(1.0)
            .duration(Duration::from_millis(500))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    ui.update_at(VIEWPORT, start + Duration::from_millis(750));
    let replaced = solid_fill(&ui, "box");
    assert!((replaced.a - 0.75).abs() < 0.01);
}

#[test]
fn style_dyn_does_not_reset_a_running_opacity_tween() {
    let cx = context();
    let red = cx.signal(false);
    let paint = red.clone();
    let mut ui = settled(
        &cx,
        div().key("box").style_dyn(move || {
            let color = if paint.get() {
                Color::rgb(255, 0, 0)
            } else {
                Color::rgb(0, 0, 255)
            };
            StyleRefinement::default()
                .width(40)
                .height(20)
                .background(color)
        }),
    );
    let id = ui.find_by_key("box").unwrap();
    let start = Instant::now();
    ui.animate(
        id,
        Motion::opacity(0.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    red.set(true);
    ui.update_at(VIEWPORT, start + Duration::from_millis(501));
    let color = solid_fill(&ui, "box");
    assert!((color.a - 0.499).abs() < 0.02);
    assert!(color.r > 0.9);
    assert_eq!(color.b, 0.0);
}

#[test]
fn a_finished_tween_stays_put_when_style_changes_later() {
    let cx = context();
    let wide = cx.signal(false);
    let width = wide.clone();
    let mut ui = settled(
        &cx,
        div().key("box").style_dyn(move || {
            StyleRefinement::default()
                .width(if width.get() { 80 } else { 40 })
                .height(20)
                .background(Color::BLACK)
        }),
    );
    let id = ui.find_by_key("box").unwrap();
    let start_y = ui.node_rect(id).unwrap().y;
    let start = Instant::now();
    ui.animate(
        id,
        Motion::translation(0.0, 30.0)
            .duration(Duration::from_millis(100))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(150));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 30.0)).abs() < 0.01);
    assert!(!ui.motion_animating());
    wide.set(true);
    ui.update_at(VIEWPORT, start + Duration::from_millis(160));
    let settled_rect = ui.node_rect(id).unwrap();
    assert!((settled_rect.y - (start_y + 30.0)).abs() < 0.01);
    assert!((settled_rect.width - 80.0).abs() < 0.01);

    ui.animate(
        id,
        Motion::translation(0.0, 0.0)
            .duration(Duration::from_millis(100))
            .ease(Ease::Linear),
    );
    let returned = start + Duration::from_millis(160);
    ui.update_at(VIEWPORT, returned);
    ui.update_at(VIEWPORT, returned + Duration::from_millis(50));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 15.0)).abs() < 0.01);
}

#[test]
fn hiding_a_node_cancels_its_tween() {
    let cx = context();
    let open = cx.signal(true);
    let shown = open.clone();
    let mut ui = settled(
        &cx,
        show(move || shown.get(), || sized_box("box", Color::BLACK)),
    );
    let id = ui.find_by_key("box").unwrap();
    let start_y = ui.node_rect(id).unwrap().y;
    let start = Instant::now();
    ui.animate(
        id,
        Motion::translation(0.0, 40.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    assert!(ui.node_rect(id).unwrap().y > start_y + 10.0);
    open.set(false);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    open.set(true);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    let again = ui.find_by_key("box").unwrap();
    assert!((ui.node_rect(again).unwrap().y - start_y).abs() < 0.01);
    assert!(!ui.motion_animating());
}

#[test]
fn disposing_a_node_drops_its_tween() {
    let cx = context();
    let left = cx.signal(true);
    let branch = left.clone();
    let mut ui = settled(
        &cx,
        if_else(
            move || branch.get(),
            || sized_box("box", Color::BLACK),
            || sized_box("box", Color::BLACK),
        ),
    );
    let id = ui.find_by_key("box").unwrap();
    let rest_y = ui.node_rect(id).unwrap().y;
    let start = Instant::now();
    ui.animate(
        id,
        Motion::translation(0.0, 40.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    left.set(false);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    ui.animate(
        id,
        Motion::translation(0.0, 80.0)
            .duration(Duration::from_millis(1000))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start + Duration::from_millis(900));
    let replacement = ui.find_by_key("box").unwrap();
    assert_ne!(replacement, id);
    assert!(!ui.motion_animating());
    assert!((ui.node_rect(replacement).unwrap().y - rest_y).abs() < 0.01);
}

#[test]
fn context_animate_reads_layout_and_shares_the_driver() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("panel", Color::BLACK));
    let start_y = ui.node_rect(ui.find_by_key("panel").unwrap()).unwrap().y;
    cx.animate(|motion| {
        let id = motion.find_by_key("panel").expect("panel");
        let height = motion.node_rect(id).unwrap().height;
        motion.animate(
            id,
            Motion::translation(0.0, height)
                .duration(Duration::from_millis(1000))
                .ease(Ease::Linear),
        );
    });
    let start = Instant::now();
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(500));
    let moved = ui.node_rect(ui.find_by_key("panel").unwrap()).unwrap();
    assert!((moved.y - (start_y + 10.0)).abs() < 0.01);
    assert!(
        ui.frame_deadline(start + Duration::from_millis(500))
            .is_some()
    );
}

#[test]
fn context_animate_wakes_the_host_once_per_batch() {
    let cx = context();
    let mut ui = settled(&cx, div());
    let wakes = Rc::new(Cell::new(0));
    let counter = wakes.clone();
    ui.set_waker(move || counter.set(counter.get() + 1));
    cx.animate(|_| {});
    assert_eq!(wakes.get(), 1);
    cx.animate(|_| {});
    assert_eq!(wakes.get(), 1);
    ui.update(VIEWPORT);
    cx.animate(|_| {});
    assert_eq!(wakes.get(), 2);
}

#[test]
#[should_panic(expected = "a context drives one Ui")]
fn a_context_has_one_ui() {
    let cx = context();
    let _first = Ui::new(&cx, div(), Theme::light());
    let _second = Ui::new(&cx, div(), Theme::light());
}
