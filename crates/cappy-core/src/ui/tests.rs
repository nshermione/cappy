use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[cfg(feature = "cpu")]
use super::motion::{Anim, Ease, Motion};
use super::*;
use crate::paint::Primitive;
use crate::reactive::{live_signal_count, signal_is_alive, signal_subscriber_count};
use crate::{
    Application, ButtonState, Component, Context, Dynamic, Element, HasContext, ImeEvent,
    InputEvent, IntoElement, Key, KeyEvent, Modifiers, PointerButton, PointerPhase, Signal,
    Singleton, SingletonOf, Slot, SurfaceCommand, SurfaceOptions, div, for_each, fragment, if_else,
    input, on_before_mount, on_before_unmount, on_mounted, on_unmounted, on_updated, show, slot,
    text, text_dyn, use_store,
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
fn a_disabled_node_ignores_hover_click_and_the_pointer() {
    let cx = context();
    let clicks = cx.reactive(0_i32);
    let clicks_handle = clicks.clone();
    let focused = cx.reactive(false);
    let focused_handle = focused.clone();
    let open = cx.reactive(false);
    let open_handle = open.clone();
    let mut theme = Theme::light();
    theme.set_class(
        "gate:disabled",
        StyleRefinement::default().background(Color::BLACK),
    );
    theme.set_class(
        "gate:hover",
        StyleRefinement::default().background(Color::WHITE),
    );
    let root = div()
        .class("gate")
        .key("gate")
        .style(StyleRefinement::default().size(40))
        .disabled(Dynamic(move || !open_handle.get()))
        .focusable()
        .on_focus(move |is_focused| focused_handle.set(is_focused))
        .on_click(move |_| clicks_handle.update(|value| *value += 1));
    let mut ui = Ui::new(&cx, root, theme);
    ui.update(VIEWPORT);
    let gate = ui.find_by_key("gate").unwrap();
    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "gate"),
    });
    ui.update(VIEWPORT);
    assert_eq!(ui.node_style(gate).unwrap().visual.background, Color::BLACK);
    assert_eq!(ui.cursor(), CursorIcon::Default);
    let point = center_of(&ui, "gate");
    click_at(&mut ui, point.x, point.y);
    key(&mut ui, Key::Tab, Modifiers::default());
    assert_eq!(clicks.get_untracked(), 0);
    assert!(!focused.get_untracked());

    open.set(true);
    ui.update(VIEWPORT);
    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "gate"),
    });
    ui.update(VIEWPORT);
    assert_eq!(ui.node_style(gate).unwrap().visual.background, Color::WHITE);
    assert_eq!(ui.cursor(), CursorIcon::Pointer);
    let point = center_of(&ui, "gate");
    click_at(&mut ui, point.x, point.y);
    assert_eq!(clicks.get_untracked(), 1);
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
fn an_image_loads_from_a_filesystem_path() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample.png");
    let cx = context();
    let ui = settled(
        &cx,
        crate::image(path)
            .key("photo")
            .style(StyleRefinement::default().size(40)),
    );
    let image = ui
        .display_list()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Image(image) => Some(image),
            Primitive::Quad(_) | Primitive::Shadow(_) | Primitive::Text(_) => None,
        })
        .expect("loaded image");
    assert_eq!((image.pixels.width(), image.pixels.height()), (32, 32));
    assert_eq!(&image.pixels.rgba()[..4], &[255, 0, 0, 128]);
}

#[test]
fn a_gif_advances_when_its_delay_elapses() {
    use crate::ImageClip;

    let clip = ImageClip::decode(include_bytes!("../../fixtures/sample.gif")).expect("gif");
    let cx = context();
    let mut ui = Ui::new(
        &cx,
        crate::image(clip)
            .key("gif")
            .style(StyleRefinement::default().size(40)),
        Theme::light(),
    );
    let start = Instant::now();
    ui.update_at(VIEWPORT, start);
    assert_eq!(gif_pixel(&ui), [255, 0, 0, 255]);
    let outcome = ui.update_at(VIEWPORT, start + Duration::from_millis(100));
    assert!(outcome.needs_redraw());
    assert_eq!(gif_pixel(&ui), [0, 0, 255, 255]);
    assert!(
        ui.frame_deadline(start + Duration::from_millis(100))
            .is_some()
    );
}

fn gif_pixel(ui: &Ui) -> [u8; 4] {
    let image = ui
        .display_list()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Image(image) => Some(image),
            Primitive::Quad(_) | Primitive::Shadow(_) | Primitive::Text(_) => None,
        })
        .expect("gif");
    let rgba = image.pixels.rgba();
    [rgba[0], rgba[1], rgba[2], rgba[3]]
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

#[test]
fn visual_motion_above_the_scroll_clip_does_not_enlarge_damage() {
    let cx = context();
    let start = Instant::now();
    let mut ui = Ui::new(&cx, clipped_motion_column(), Theme::light());
    ui.update_at(VIEWPORT, start);
    let scroller_id = ui.find_by_key("scroller").unwrap();
    let box_id = ui.find_by_key("box").unwrap();
    ui.jump_scroll(scroller_id, true, 20.0);
    ui.update_at(VIEWPORT, start);
    ui.prepare_frame();

    let scroller = ui.node_rect(scroller_id).unwrap();
    let hidden = ui.node_rect(box_id).unwrap();
    assert!(hidden.y < scroller.y);
    assert!(hidden.bottom() > scroller.y);

    ui.animate(
        box_id,
        Motion::background(Color::rgb(220, 40, 40)).duration(Duration::from_millis(200)),
    );
    let stepped = ui.update_at(VIEWPORT, start + Duration::from_millis(40));
    let UpdateOutcome::Redraw { damage } = stepped else {
        panic!("expected a redraw, got {stepped:?}");
    };
    let regions = damage.rects(VIEWPORT.logical_size());
    assert!(!regions.is_empty());
    for region in &regions {
        assert_eq!(
            region.intersection(&scroller),
            Some(*region),
            "hidden pixels above the clip are in {region:?}, scroller {scroller:?}"
        );
    }
}

fn clipped_motion_column() -> Element {
    div().child(
        div()
            .key("scroller")
            .style(
                StyleRefinement::default()
                    .direction(FlexDirection::Column)
                    .width(120)
                    .height(80)
                    .overflow_y(Overflow::Auto),
            )
            .children([
                div().key("box").style(
                    StyleRefinement::default()
                        .width(100)
                        .height(40)
                        .flex_shrink(0.0)
                        .background(Color::BLACK),
                ),
                div().style(
                    StyleRefinement::default()
                        .width(100)
                        .height(200)
                        .flex_shrink(0.0),
                ),
            ]),
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
fn a_loop_wraps_and_a_ping_pong_comes_back() {
    let start = Instant::now();
    {
        let cx = context();
        let mut ui = settled(&cx, sized_box("box", Color::BLACK));
        let id = ui.find_by_key("box").unwrap();
        let start_y = ui.node_rect(id).unwrap().y;
        ui.animate(
            id,
            Motion::translation(0.0, 100.0)
                .duration(Duration::from_millis(100))
                .ease(Ease::Linear)
                .looped(),
        );
        ui.update_at(VIEWPORT, start);
        ui.update_at(VIEWPORT, start + Duration::from_millis(150));
        assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
        assert!(ui.motion_animating());
    }

    let cx = context();
    let mut ui = settled(&cx, sized_box("pong", Color::BLACK));
    let id = ui.find_by_key("pong").unwrap();
    let start_y = ui.node_rect(id).unwrap().y;
    ui.animate(
        id,
        Motion::translation(0.0, 100.0)
            .duration(Duration::from_millis(100))
            .ease(Ease::Linear)
            .ping_pong(),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(100));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 100.0)).abs() < 0.01);
    ui.update_at(VIEWPORT, start + Duration::from_millis(150));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
    assert!(ui.motion_animating());

    let cx = context();
    let mut ui = settled(&cx, sized_box("pong-ease", Color::BLACK));
    let id = ui.find_by_key("pong-ease").unwrap();
    let start_y = ui.node_rect(id).unwrap().y;
    ui.animate(
        id,
        Motion::translation(0.0, 100.0)
            .duration(Duration::from_millis(100))
            .ease(Ease::OutQuad)
            .ping_pong(),
    );
    ui.update_at(VIEWPORT, start);
    // Halfway back is a new OutQuad from the destination, not the outward curve reversed.
    // OutQuad(0.5) = 0.75, so three quarters of the way home.
    ui.update_at(VIEWPORT, start + Duration::from_millis(150));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 25.0)).abs() < 0.01);
}

#[test]
fn delay_holds_the_start_until_it_elapses() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("box", Color::BLACK));
    let id = ui.find_by_key("box").unwrap();
    let start_y = ui.node_rect(id).unwrap().y;
    let start = Instant::now();
    ui.animate(
        id,
        Motion::translation(0.0, 100.0)
            .duration(Duration::from_millis(100))
            .delay(Duration::from_millis(100))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(100));
    assert!((ui.node_rect(id).unwrap().y - start_y).abs() < 0.01);
    ui.update_at(VIEWPORT, start + Duration::from_millis(150));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
}

#[test]
fn sequence_waits_and_together_starts_at_once() {
    let start = Instant::now();
    {
        let cx = context();
        let mut ui = settled(&cx, div().children([sized_box("box", Color::BLACK)]));
        let id = ui.find_by_key("box").unwrap();
        let start_y = ui.node_rect(id).unwrap().y;
        ui.play(Anim::sequence([
            Anim::play(
                id,
                Motion::translation(0.0, 100.0)
                    .duration(Duration::from_millis(100))
                    .ease(Ease::Linear),
            ),
            Anim::play(
                id,
                Motion::translation(0.0, 0.0)
                    .duration(Duration::from_millis(100))
                    .ease(Ease::Linear),
            ),
        ]));
        ui.update_at(VIEWPORT, start);
        ui.update_at(VIEWPORT, start + Duration::from_millis(50));
        assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
        ui.update_at(VIEWPORT, start + Duration::from_millis(100));
        assert!((ui.node_rect(id).unwrap().y - (start_y + 100.0)).abs() < 0.01);
        ui.update_at(VIEWPORT, start + Duration::from_millis(150));
        assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
    }

    let cx = context();
    let mut ui = settled(
        &cx,
        div().children([
            sized_box("left", Color::BLACK),
            sized_box("right", Color::BLACK),
        ]),
    );
    let left = ui.find_by_key("left").unwrap();
    let right = ui.find_by_key("right").unwrap();
    let left_y = ui.node_rect(left).unwrap().y;
    ui.play(Anim::together([
        Anim::play(
            left,
            Motion::translation(0.0, 100.0)
                .duration(Duration::from_millis(100))
                .ease(Ease::Linear),
        ),
        Anim::play(
            right,
            Motion::opacity(0.0)
                .duration(Duration::from_millis(100))
                .ease(Ease::Linear),
        ),
    ]));
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(50));
    assert!((ui.node_rect(left).unwrap().y - (left_y + 50.0)).abs() < 0.01);
    assert!((ui.nodes[right.index()].opacity - 0.5).abs() < 0.01);
}

#[test]
fn stopping_a_play_handle_holds_the_value_and_skips_the_rest() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("box", Color::BLACK));
    let id = ui.find_by_key("box").unwrap();
    let start_y = ui.node_rect(id).unwrap().y;
    let start = Instant::now();
    let handle = ui.play(Anim::sequence([
        Anim::play(
            id,
            Motion::translation(0.0, 100.0)
                .duration(Duration::from_millis(100))
                .ease(Ease::Linear),
        ),
        Anim::play(
            id,
            Motion::translation(0.0, 0.0)
                .duration(Duration::from_millis(100))
                .ease(Ease::Linear),
        ),
    ]));
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(50));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
    handle.stop();
    handle.stop();
    ui.update_at(VIEWPORT, start + Duration::from_millis(200));
    assert!((ui.node_rect(id).unwrap().y - (start_y + 50.0)).abs() < 0.01);
    assert!(!ui.motion_animating());
}

#[test]
fn scale_grows_the_painted_box_and_the_hit_target() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("box", Color::BLACK));
    let id = ui.find_by_key("box").unwrap();
    let layout = ui.node_rect(id).unwrap();
    let start = Instant::now();
    ui.animate(
        id,
        Motion::scale(2.0)
            .duration(Duration::from_millis(100))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(50));
    assert!((ui.node_rect(id).unwrap().width - layout.width).abs() < 0.01);
    let painted = painted_quad(&ui, "box");
    assert!((painted.rect.width - layout.width * 1.5).abs() < 0.01);
    let inside = Point::new(layout.x - 4.0, layout.y + 1.0);
    assert!(ui.hit_path(inside).contains(&id));
    let outside = Point::new(layout.x - layout.width, layout.y + 1.0);
    assert!(!ui.hit_path(outside).contains(&id));
}

#[test]
fn background_lerps_and_rotation_spins_the_quad() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("box", Color::BLACK));
    let id = ui.find_by_key("box").unwrap();
    let start = Instant::now();
    ui.animate(
        id,
        Motion::background(Color::WHITE)
            .duration(Duration::from_millis(100))
            .ease(Ease::Linear),
    );
    ui.animate(
        id,
        Motion::rotation(180.0)
            .duration(Duration::from_millis(100))
            .ease(Ease::Linear),
    );
    ui.update_at(VIEWPORT, start);
    ui.update_at(VIEWPORT, start + Duration::from_millis(50));
    let color = solid_fill(&ui, "box");
    assert!((color.r - 0.5).abs() < 0.02, "red channel {}", color.r);
    let spin = painted_quad(&ui, "box").spin.radians;
    assert!((spin - std::f32::consts::FRAC_PI_2).abs() < 0.02);
}

#[test]
fn shake_offsets_sideways_for_one_cycle() {
    let cx = context();
    let mut ui = settled(&cx, sized_box("box", Color::BLACK));
    let id = ui.find_by_key("box").unwrap();
    let start_x = ui.node_rect(id).unwrap().x;
    let start = Instant::now();
    ui.animate(
        id,
        Motion::shake(10.0)
            .duration(Duration::from_millis(100))
            .looped(),
    );
    ui.update_at(VIEWPORT, start);
    assert!((ui.node_rect(id).unwrap().x - start_x).abs() < 0.01);
    ui.update_at(VIEWPORT, start + Duration::from_millis(25));
    assert!((ui.node_rect(id).unwrap().x - (start_x + 10.0)).abs() < 0.01);
    assert!(ui.motion_animating());
}

fn painted_quad<'a>(ui: &'a Ui, key: &str) -> &'a crate::paint::QuadPrimitive {
    let layout = ui.node_rect(ui.find_by_key(key).unwrap()).unwrap();
    let center_x = layout.x + layout.width * 0.5;
    let center_y = layout.y + layout.height * 0.5;
    ui.display_list()
        .iter()
        .find_map(|primitive| {
            let Primitive::Quad(quad) = primitive else {
                return None;
            };
            let x = quad.rect.x + quad.rect.width * 0.5;
            let y = quad.rect.y + quad.rect.height * 0.5;
            ((x - center_x).abs() < 0.5 && (y - center_y).abs() < 0.5).then_some(quad)
        })
        .expect("painted quad")
}

#[test]
fn two_uis_share_a_signal_and_surface_commands_drain_once() {
    let cx = context();
    let count = cx.reactive(0_i32);
    let primary_wakes = Rc::new(Cell::new(0));
    let secondary_wakes = Rc::new(Cell::new(0));
    let mut first = Ui::new(
        &cx,
        text_dyn({
            let count = count.clone();
            move || count.get().to_string()
        }),
        Theme::light(),
    );
    let mut second = Ui::new(
        &cx,
        text_dyn({
            let count = count.clone();
            move || count.get().to_string()
        }),
        Theme::light(),
    );
    let primary_flag = Rc::clone(&primary_wakes);
    first.set_waker(move || primary_flag.set(primary_flag.get() + 1));
    let secondary_flag = Rc::clone(&secondary_wakes);
    second.set_waker(move || secondary_flag.set(secondary_flag.get() + 1));
    first.update(VIEWPORT);
    second.update(VIEWPORT);
    count.set(4);
    assert!(first.needs_update());
    assert!(second.needs_update());
    assert_eq!(primary_wakes.get(), 1);
    assert_eq!(secondary_wakes.get(), 1);

    let id = cx.open_surface(SurfaceOptions::new("popup"), |_| div());
    assert_eq!(primary_wakes.get(), 2);
    assert_eq!(secondary_wakes.get(), 1);
    match cx.drain_surfaces().as_slice() {
        [
            SurfaceCommand::Open {
                id: opened,
                options,
                ..
            },
        ] => {
            assert_eq!(*opened, id);
            assert_eq!(options.title, "popup");
            assert_eq!(options.logical_size, (360.0, 240.0));
            assert_eq!(options.offset, (32.0, 32.0));
        }
        other => panic!("expected one open command, got {other:?}"),
    }
    assert!(cx.drain_surfaces().is_empty());
    cx.close_surface(id);
    assert!(matches!(
        cx.drain_surfaces().as_slice(),
        [SurfaceCommand::Close(closed)] if *closed == id
    ));
    assert!(cx.drain_surfaces().is_empty());
}

fn phase(log: &Rc<RefCell<Vec<String>>>, label: &'static str) -> impl FnMut() + 'static {
    let log = Rc::clone(log);
    move || log.borrow_mut().push(label.to_owned())
}

#[derive(Clone)]
struct HookChild {
    context: Context,
    log: Rc<RefCell<Vec<String>>>,
}

impl HasContext for HookChild {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for HookChild {
    fn render(&self) -> Element {
        on_before_mount(phase(&self.log, "child:before_mount"));
        on_mounted(phase(&self.log, "child:mounted"));
        on_mounted(|| use_store(SingletonOf::<HookNote>::new()).value.set(4));
        on_before_unmount(phase(&self.log, "child:before_unmount"));
        on_unmounted(phase(&self.log, "child:unmounted"));
        div().key("child")
    }
}

#[derive(Clone)]
struct HookNote {
    value: Signal<i32>,
}

impl Singleton for HookNote {
    fn create(cx: &Context) -> Self {
        Self {
            value: cx.signal(0),
        }
    }
}

#[derive(Clone)]
struct HookParent {
    context: Context,
    log: Rc<RefCell<Vec<String>>>,
}

impl HasContext for HookParent {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for HookParent {
    fn render(&self) -> Element {
        on_before_mount(phase(&self.log, "parent:before_mount"));
        on_before_mount(phase(&self.log, "parent:before_mount_2"));
        on_mounted(phase(&self.log, "parent:mounted"));
        on_before_unmount(phase(&self.log, "parent:before_unmount"));
        on_unmounted(phase(&self.log, "parent:unmounted"));
        div().key("parent").child(HookChild {
            context: self.context.clone(),
            log: Rc::clone(&self.log),
        })
    }
}

#[derive(Clone)]
struct HookChain {
    context: Context,
    log: Rc<RefCell<Vec<String>>>,
}

impl HasContext for HookChain {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for HookChain {
    fn render(&self) -> Element {
        on_before_mount(phase(&self.log, "chain:before_mount"));
        on_mounted(phase(&self.log, "chain:mounted"));
        on_before_unmount(phase(&self.log, "chain:before_unmount"));
        on_unmounted(phase(&self.log, "chain:unmounted"));
        HookChild {
            context: self.context.clone(),
            log: Rc::clone(&self.log),
        }
        .into_element()
    }
}

#[derive(Clone)]
struct HookFragment {
    context: Context,
    log: Rc<RefCell<Vec<String>>>,
}

impl HasContext for HookFragment {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for HookFragment {
    fn render(&self) -> Element {
        on_mounted(phase(&self.log, "fragment:mounted"));
        on_unmounted(phase(&self.log, "fragment:unmounted"));
        fragment([text("a").key("a"), text("b").key("b")])
    }
}

#[derive(Clone)]
struct HookTicker {
    context: Context,
    ticks: Rc<Cell<u32>>,
    value: Signal<i32>,
}

impl HasContext for HookTicker {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for HookTicker {
    fn render(&self) -> Element {
        let ticks = Rc::clone(&self.ticks);
        on_updated(move || ticks.set(ticks.get() + 1));
        let value = self.value.clone();
        on_updated(move || value.set(value.get_untracked() + 1));
        let value = self.value.clone();
        text_dyn(move || value.get().to_string()).key("value")
    }
}

#[derive(Clone)]
struct HookQuiet {
    context: Context,
    mounts: Rc<Cell<u32>>,
}

impl HasContext for HookQuiet {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for HookQuiet {
    fn render(&self) -> Element {
        let mounts = Rc::clone(&self.mounts);
        on_mounted(move || mounts.set(mounts.get() + 1));
        div().key("quiet")
    }
}

#[test]
fn component_hooks_run_parent_before_child_and_unmount_in_reverse() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let ui = Ui::new(
            &cx,
            HookParent {
                context: cx.clone(),
                log: Rc::clone(&log),
            },
            Theme::light(),
        );
        assert_eq!(
            log.borrow().as_slice(),
            [
                "parent:before_mount",
                "parent:before_mount_2",
                "child:before_mount",
                "child:mounted",
                "parent:mounted",
            ]
        );
        assert_eq!(ui.resources().live_components, 2);
        let note = cx.enter(|| {
            use_store(SingletonOf::<HookNote>::new())
                .value
                .get_untracked()
        });
        assert_eq!(note, 4);
        assert!(ui.find_by_key("child").is_some());
    }
    assert_eq!(
        log.borrow().as_slice(),
        [
            "parent:before_mount",
            "parent:before_mount_2",
            "child:before_mount",
            "child:mounted",
            "parent:mounted",
            "parent:before_unmount",
            "child:before_unmount",
            "child:unmounted",
            "parent:unmounted",
        ]
    );
}

#[test]
fn component_that_returns_a_child_component_keeps_both_lifecycles() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let ui = Ui::new(
            &cx,
            HookChain {
                context: cx.clone(),
                log: Rc::clone(&log),
            },
            Theme::light(),
        );
        assert_eq!(
            log.borrow().as_slice(),
            [
                "chain:before_mount",
                "child:before_mount",
                "child:mounted",
                "chain:mounted",
            ]
        );
        assert_eq!(ui.resources().live_components, 2);
    }
    assert_eq!(
        log.borrow().as_slice(),
        [
            "chain:before_mount",
            "child:before_mount",
            "child:mounted",
            "chain:mounted",
            "chain:before_unmount",
            "child:before_unmount",
            "child:unmounted",
            "chain:unmounted",
        ]
    );
}

#[test]
fn component_fragment_root_still_mounts_its_hooks() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    {
        let ui = Ui::new(
            &cx,
            HookFragment {
                context: cx.clone(),
                log: Rc::clone(&log),
            },
            Theme::light(),
        );
        assert_eq!(log.borrow().as_slice(), ["fragment:mounted"]);
        assert_eq!(ui.node_text(ui.find_by_key("a").unwrap()).unwrap(), "a");
        assert_eq!(ui.node_text(ui.find_by_key("b").unwrap()).unwrap(), "b");
        assert_eq!(ui.resources().live_components, 1);
    }
    assert_eq!(
        log.borrow().as_slice(),
        ["fragment:mounted", "fragment:unmounted"]
    );
}

#[test]
fn component_without_hooks_adds_no_boundary() {
    #[derive(Clone)]
    struct Plain {
        context: Context,
    }

    impl HasContext for Plain {
        fn context(&self) -> &Context {
            &self.context
        }
    }

    impl Component for Plain {
        fn render(&self) -> Element {
            div().key("plain")
        }
    }

    let cx = context();
    let ui = Ui::new(
        &cx,
        Plain {
            context: cx.clone(),
        },
        Theme::light(),
    );
    assert_eq!(ui.resources().live_components, 0);
    assert_eq!(ui.resources().components, 0);
    assert!(ui.find_by_key("plain").is_some());
}

#[test]
fn show_hides_a_component_without_unmounting_and_keeps_its_frame_tick() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    let ticks = Rc::new(Cell::new(0));
    let shown = cx.signal(true);
    let flag = shown.clone();
    let log_view = Rc::clone(&log);
    let ticks_view = Rc::clone(&ticks);
    let cx_view = cx.clone();
    let mut ui = settled(
        &cx,
        div().child(show(
            move || flag.get(),
            move || {
                HookShown {
                    context: cx_view.clone(),
                    log: Rc::clone(&log_view),
                    ticks: Rc::clone(&ticks_view),
                }
                .into_element()
            },
        )),
    );
    assert_eq!(log.borrow().as_slice(), ["shown:mounted"]);
    assert_eq!(ticks.get(), 1);
    shown.set(false);
    ui.update(VIEWPORT);
    assert_eq!(log.borrow().as_slice(), ["shown:mounted"]);
    assert_eq!(ticks.get(), 2);
    assert!(ui.frame_deadline(Instant::now()).is_some());
    shown.set(true);
    ui.update(VIEWPORT);
    assert_eq!(log.borrow().as_slice(), ["shown:mounted"]);
    assert_eq!(ticks.get(), 3);

    #[derive(Clone)]
    struct HookShown {
        context: Context,
        log: Rc<RefCell<Vec<String>>>,
        ticks: Rc<Cell<u32>>,
    }

    impl HasContext for HookShown {
        fn context(&self) -> &Context {
            &self.context
        }
    }

    impl Component for HookShown {
        fn render(&self) -> Element {
            on_mounted(phase(&self.log, "shown:mounted"));
            on_unmounted(phase(&self.log, "shown:unmounted"));
            let ticks = Rc::clone(&self.ticks);
            on_updated(move || ticks.set(ticks.get() + 1));
            div().key("shown")
        }
    }
}

#[test]
fn if_branch_unmounts_the_component_it_drops_and_releases_its_slot() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    let on = cx.signal(false);
    let flag = on.clone();
    let log_then = Rc::clone(&log);
    let log_else = Rc::clone(&log);
    let cx_then = cx.clone();
    let cx_else = cx.clone();
    let mut ui = settled(
        &cx,
        div().child(if_else(
            move || flag.get(),
            move || {
                HookBranch {
                    context: cx_then.clone(),
                    log: Rc::clone(&log_then),
                    name: "then",
                }
                .into_element()
            },
            move || {
                HookBranch {
                    context: cx_else.clone(),
                    log: Rc::clone(&log_else),
                    name: "else",
                }
                .into_element()
            },
        )),
    );
    let baseline = ui.resources();
    assert_eq!(log.borrow().as_slice(), ["else:mounted"]);
    assert_eq!(baseline.live_components, 1);
    on.set(true);
    ui.update(VIEWPORT);
    assert_eq!(
        log.borrow().as_slice(),
        ["else:mounted", "else:unmounted", "then:mounted"]
    );
    on.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ui.resources(), baseline);
    assert_eq!(
        log.borrow().as_slice(),
        [
            "else:mounted",
            "else:unmounted",
            "then:mounted",
            "then:unmounted",
            "else:mounted",
        ]
    );

    #[derive(Clone)]
    struct HookBranch {
        context: Context,
        log: Rc<RefCell<Vec<String>>>,
        name: &'static str,
    }

    impl HasContext for HookBranch {
        fn context(&self) -> &Context {
            &self.context
        }
    }

    impl Component for HookBranch {
        fn render(&self) -> Element {
            let name = self.name;
            on_mounted(phase(
                &self.log,
                match name {
                    "then" => "then:mounted",
                    _ => "else:mounted",
                },
            ));
            on_unmounted(phase(
                &self.log,
                match name {
                    "then" => "then:unmounted",
                    _ => "else:unmounted",
                },
            ));
            div().key(name)
        }
    }
}

#[test]
fn on_updated_reruns_every_frame_and_flushes_a_signal_in_that_update() {
    let cx = context();
    let ticks = Rc::new(Cell::new(0));
    let mounts = Rc::new(Cell::new(0));
    let value = cx.signal(0);
    let mut ui = Ui::new(
        &cx,
        div().children([
            HookTicker {
                context: cx.clone(),
                ticks: Rc::clone(&ticks),
                value: value.clone(),
            }
            .into_element(),
            HookQuiet {
                context: cx.clone(),
                mounts: Rc::clone(&mounts),
            }
            .into_element(),
        ]),
        Theme::light(),
    );
    assert_eq!(ticks.get(), 0);
    assert_eq!(mounts.get(), 1);
    assert_eq!(ui.node_text(ui.find_by_key("value").unwrap()).unwrap(), "0");
    ui.update(VIEWPORT);
    assert_eq!(ticks.get(), 1);
    assert_eq!(mounts.get(), 1);
    assert_eq!(ui.node_text(ui.find_by_key("value").unwrap()).unwrap(), "1");
    assert!(ui.frame_deadline(Instant::now()).is_some());
    ui.update(VIEWPORT);
    assert_eq!(ticks.get(), 2);
    assert_eq!(ui.node_text(ui.find_by_key("value").unwrap()).unwrap(), "2");

    let on = cx.signal(true);
    let flag = on.clone();
    let ticks_branch = Rc::clone(&ticks);
    let value_branch = value.clone();
    let cx_branch = cx.clone();
    drop(ui);
    let mut ui = settled(
        &cx,
        div().child(if_else(
            move || flag.get(),
            {
                let ticks_branch = Rc::clone(&ticks_branch);
                let value_branch = value_branch.clone();
                let cx_branch = cx_branch.clone();
                move || {
                    HookTicker {
                        context: cx_branch.clone(),
                        ticks: Rc::clone(&ticks_branch),
                        value: value_branch.clone(),
                    }
                    .into_element()
                }
            },
            || div().key("gone"),
        )),
    );
    let after_mount = ticks.get();
    assert!(ui.frame_deadline(Instant::now()).is_some());
    assert!(ui.find_by_key("value").is_some());
    on.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ticks.get(), after_mount);
    assert!(ui.frame_deadline(Instant::now()).is_none());
    ui.update(VIEWPORT);
    assert_eq!(ticks.get(), after_mount);
    assert!(ui.find_by_key("gone").is_some());
}

#[test]
#[should_panic(expected = "on_mounted called outside a component render")]
fn on_mounted_outside_render_panics() {
    let cx = context();
    cx.enter(|| on_mounted(|| {}));
}

fn field(value: impl crate::IntoInputValue) -> (Element, Rc<RefCell<String>>) {
    let seen = Rc::new(RefCell::new(String::new()));
    let slot = Rc::clone(&seen);
    let element = input()
        .value(value)
        .key("field")
        .style(StyleRefinement::default().width(200).font_size(10))
        .on_input(move |text| *slot.borrow_mut() = text.to_owned());
    (element, seen)
}

fn click_at(ui: &mut Ui, x: f32, y: f32) {
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(x, y),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
}

fn key(ui: &mut Ui, key: Key, modifiers: Modifiers) {
    ui.handle_event(InputEvent::Key(KeyEvent {
        key,
        state: ButtonState::Pressed,
        repeat: false,
        modifiers,
    }));
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

#[test]
fn dynamic_text_reads_a_reactive_string() {
    let cx = context();
    let name = cx.reactive(String::from("ada"));
    let mut ui = Ui::new(
        &cx,
        text(Dynamic(move || name)).key("label"),
        Theme::light(),
    );
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("ada"));
    name.set(String::from("bea"));
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("bea"));
}

#[test]
fn tab_focuses_the_input_and_typing_appends_at_the_caret() {
    let cx = context();
    let name = cx.reactive(String::from("hi"));
    let mut ui = Ui::new(
        &cx,
        input()
            .value(Dynamic(move || name))
            .key("field")
            .style(StyleRefinement::default().width(200).font_size(10)),
        Theme::light(),
    );
    ui.update(VIEWPORT);
    key(&mut ui, Key::Tab, Modifiers::default());
    ui.handle_event(InputEvent::Text("!".into()));
    ui.update(VIEWPORT);
    assert_eq!(name.get(), "hi!");
}

#[test]
fn click_between_letters_inserts_there() {
    let (element, seen) = field("hello");
    let mut ui = settled(&context(), element);
    click_at(&mut ui, 11.0, 4.0);
    ui.handle_event(InputEvent::Text("XX".into()));
    assert_eq!(seen.borrow().as_str(), "heXXllo");
}

#[test]
fn click_before_the_first_letter_inserts_there() {
    let (element, seen) = field("hello");
    let mut ui = settled(&context(), element);
    click_at(&mut ui, 0.0, 4.0);
    ui.handle_event(InputEvent::Text("A".into()));
    assert_eq!(seen.borrow().as_str(), "Ahello");
}

#[test]
fn click_after_the_last_letter_inserts_there() {
    let (element, seen) = field("hello");
    let mut ui = settled(&context(), element);
    click_at(&mut ui, 80.0, 4.0);
    ui.handle_event(InputEvent::Text("Z".into()));
    assert_eq!(seen.borrow().as_str(), "helloZ");
}

#[test]
fn drag_selects_a_range_and_copy_reads_only_that_range() {
    let (element, _) = field("hello");
    let mut ui = settled(&context(), element);
    ui.clipboard_mut().write_text("keep");
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(6.0, 4.0),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(16.0, 4.0),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    key(&mut ui, Key::Character('c'), ctrl());
    assert_eq!(ui.clipboard_mut().read_text().as_deref(), Some("el"));
}

#[test]
fn copy_without_a_selection_leaves_the_clipboard_alone() {
    let (element, _) = field("hello");
    let mut ui = settled(&context(), element);
    ui.clipboard_mut().write_text("keep");
    click_at(&mut ui, 11.0, 4.0);
    key(&mut ui, Key::Character('c'), ctrl());
    assert_eq!(ui.clipboard_mut().read_text().as_deref(), Some("keep"));
}

#[test]
fn paste_inserts_at_the_caret_and_replaces_a_selection() {
    let (element, seen) = field("hello");
    let mut ui = settled(&context(), element);
    click_at(&mut ui, 11.0, 4.0);
    ui.clipboard_mut().write_text("XX");
    key(&mut ui, Key::Character('v'), ctrl());
    assert_eq!(seen.borrow().as_str(), "heXXllo");
}

#[test]
fn paste_replaces_the_selected_range() {
    let (element, seen) = field("hello");
    let mut ui = settled(&context(), element);
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(11.0, 4.0),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(20.0, 4.0),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    ui.clipboard_mut().write_text("XX");
    key(&mut ui, Key::Character('v'), ctrl());
    assert_eq!(seen.borrow().as_str(), "heXXo");
}

#[test]
fn select_all_and_shift_arrow_copy_the_highlighted_text() {
    let (element, _) = field("hello");
    let mut ui = settled(&context(), element);
    click_at(&mut ui, 0.0, 4.0);
    key(&mut ui, Key::Character('a'), ctrl());
    key(&mut ui, Key::Character('c'), ctrl());
    assert_eq!(ui.clipboard_mut().read_text().as_deref(), Some("hello"));

    key(&mut ui, Key::Home, Modifiers::default());
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    key(&mut ui, Key::Right, shift);
    key(&mut ui, Key::Right, shift);
    key(&mut ui, Key::Character('c'), ctrl());
    assert_eq!(ui.clipboard_mut().read_text().as_deref(), Some("he"));
}

#[test]
fn backspace_deletes_the_selection_or_the_grapheme_before_the_caret() {
    let (element, seen) = field("hếllo");
    let mut ui = settled(&context(), element);
    click_at(&mut ui, 80.0, 4.0);
    key(&mut ui, Key::Backspace, Modifiers::default());
    assert_eq!(seen.borrow().as_str(), "hếll");
}

#[test]
fn ime_commit_inserts_at_the_caret() {
    let cx = context();
    let name = cx.reactive(String::from("he"));
    let mut ui = Ui::new(
        &cx,
        input()
            .value(Dynamic(move || name))
            .key("field")
            .style(StyleRefinement::default().width(200).font_size(10)),
        Theme::light(),
    );
    ui.update(VIEWPORT);
    key(&mut ui, Key::Tab, Modifiers::default());
    ui.handle_event(InputEvent::Ime(ImeEvent::Preedit {
        text: "ế".into(),
        cursor: None,
    }));
    ui.update(VIEWPORT);
    assert_eq!(name.get(), "he");
    ui.handle_event(InputEvent::Ime(ImeEvent::Commit("ế".into())));
    ui.handle_event(InputEvent::Text("ế".into()));
    ui.update(VIEWPORT);
    assert_eq!(name.get(), "heế");
}

#[test]
fn a_disabled_input_does_not_take_focus_or_text() {
    let (element, seen) = field("hi");
    let element = element.disabled(true);
    let mut ui = settled(&context(), element);
    key(&mut ui, Key::Tab, Modifiers::default());
    ui.handle_event(InputEvent::Text("!".into()));
    assert_eq!(seen.borrow().as_str(), "");
}

#[test]
fn external_signal_writes_replace_the_buffer_and_keep_a_caret_inside_it() {
    let cx = context();
    let name = cx.reactive(String::from("hello"));
    let mut ui = Ui::new(
        &cx,
        input()
            .value(Dynamic(move || name))
            .key("field")
            .style(StyleRefinement::default().width(200).font_size(10)),
        Theme::light(),
    );
    ui.update(VIEWPORT);
    click_at(&mut ui, 11.0, 4.0);
    name.set(String::from("yo"));
    ui.update(VIEWPORT);
    ui.handle_event(InputEvent::Text("!".into()));
    ui.update(VIEWPORT);
    assert_eq!(name.get(), "yo!");
}

fn rect_label(cx: &Context, key: &'static str) -> Element {
    let read = cx.clone();
    text_dyn(move || {
        read.node_rect(key)
            .map(|rect| format!("{}", rect.width as i32))
            .unwrap_or_else(|| "none".into())
    })
}

#[test]
fn a_binding_reads_the_laid_out_rect_and_updates_when_it_changes() {
    let cx = context();
    let width = cx.signal(100);
    let width_style = width.clone();
    let mut ui = settled(
        &cx,
        div()
            .key("box")
            .style_dyn(move || {
                StyleRefinement::default()
                    .width(width_style.get())
                    .height(40)
                    .flex_shrink(0.0)
            })
            .child(rect_label(&cx, "box").key("label")),
    );
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("100"));
    assert_eq!(
        ui.node_rect(ui.find_by_key("box").unwrap()).unwrap().width,
        100.0
    );

    width.set(180);
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("180"));
    assert!(!ui.needs_update());
}

#[test]
fn a_missing_key_reads_none_and_does_not_keep_the_ui_awake() {
    let cx = context();
    let ui = settled(&cx, rect_label(&cx, "missing").key("label"));
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("none"));
    assert!(!ui.needs_update());
}

#[test]
fn disposing_a_keyed_node_clears_the_rect_a_binding_reads() {
    let cx = context();
    let show_box = cx.signal(true);
    let visible = show_box.clone();
    let mut ui = settled(
        &cx,
        div().children([
            if_else(
                move || visible.get(),
                || {
                    div().key("box").style(
                        StyleRefinement::default()
                            .width(70)
                            .height(20)
                            .flex_shrink(0.0),
                    )
                },
                || div(),
            ),
            rect_label(&cx, "box").key("label"),
        ]),
    );
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("70"));
    show_box.set(false);
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(ui.find_by_key("label").unwrap()), Some("none"));
    assert!(ui.find_by_key("box").is_none());
}

#[test]
fn set_scroll_offset_jumps_wakes_the_host_and_a_binding_reads_it() {
    let cx = context();
    let read = cx.clone();
    let mut ui = settled(
        &cx,
        div().children([
            scroll_column(Overflow::Auto),
            text_dyn(move || {
                read.scroll_offset("scroller")
                    .map(|offset| format!("{}", offset.y as i32))
                    .unwrap_or_else(|| "none".into())
            })
            .key("offset"),
        ]),
    );
    assert_eq!(ui.node_text(ui.find_by_key("offset").unwrap()), Some("0"));
    let before = ui.node_rect(ui.find_by_key("tall").unwrap()).unwrap().y;

    let wakes = Rc::new(Cell::new(0));
    let flag = Rc::clone(&wakes);
    ui.set_waker(move || flag.set(flag.get() + 1));
    cx.set_scroll_offset("scroller", Point::new(0.0, 30.0));
    assert_eq!(wakes.get(), 1);
    assert!(ui.needs_update());

    ui.update(VIEWPORT);
    let after = ui.node_rect(ui.find_by_key("tall").unwrap()).unwrap().y;
    assert!((before - after - 30.0).abs() < 0.5);
    assert_eq!(ui.node_text(ui.find_by_key("offset").unwrap()), Some("30"));
    assert!(!ui.scroll_animating());
}

#[test]
fn a_rect_binding_does_not_rerun_when_only_scroll_changes() {
    let cx = context();
    let hits = Rc::new(Cell::new(0));
    let count = Rc::clone(&hits);
    let read = cx.clone();
    let mut ui = settled(
        &cx,
        div().children([
            scroll_column(Overflow::Auto),
            text_dyn(move || {
                count.set(count.get() + 1);
                read.node_rect("scroller")
                    .map(|rect| format!("{}", rect.height as i32))
                    .unwrap_or_else(|| "none".into())
            })
            .key("height"),
        ]),
    );
    let after_layout = hits.get();
    assert!(after_layout >= 1);
    assert_eq!(ui.node_text(ui.find_by_key("height").unwrap()), Some("80"));
    cx.set_scroll_offset("scroller", Point::new(0.0, 25.0));
    ui.update(VIEWPORT);
    assert_eq!(hits.get(), after_layout);
}

#[test]
fn a_click_reads_the_node_rect_and_writes_its_scroll_offset() {
    let cx = context();
    let seen = Rc::new(Cell::new(None));
    let seen_click = Rc::clone(&seen);
    let read = cx.clone();
    let write = cx.clone();
    let mut ui = settled(
        &cx,
        div().children([
            scroll_column(Overflow::Auto),
            div()
                .key("back")
                .style(
                    StyleRefinement::default()
                        .width(40)
                        .height(20)
                        .flex_shrink(0.0),
                )
                .on_click(move |_| {
                    seen_click.set(read.node_rect("back"));
                    write.set_scroll_offset("scroller", Point::new(0.0, 40.0));
                })
                .child(text("top")),
        ]),
    );
    let before = ui.node_rect(ui.find_by_key("tall").unwrap()).unwrap().y;
    click(&mut ui, "back");
    let reported = seen.get().expect("click saw the button rect");
    assert_eq!(
        reported,
        ui.node_rect(ui.find_by_key("back").unwrap()).unwrap()
    );
    assert!(ui.needs_update());
    ui.update(VIEWPORT);
    let after = ui.node_rect(ui.find_by_key("tall").unwrap()).unwrap().y;
    assert!((before - after - 40.0).abs() < 0.5);
}

#[test]
fn two_uis_keep_separate_geometry() {
    let cx = context();
    let mut first = settled(&cx, sized_geometry(&cx, 40));
    let mut second = settled(&cx, sized_geometry(&cx, 90));
    second.update(VIEWPORT);
    first.update(VIEWPORT);
    assert_eq!(
        first.node_text(first.find_by_key("label").unwrap()),
        Some("40")
    );
    assert_eq!(
        second.node_text(second.find_by_key("label").unwrap()),
        Some("90")
    );

    let primary_wakes = Rc::new(Cell::new(0));
    let primary_flag = Rc::clone(&primary_wakes);
    first.set_waker(move || primary_flag.set(primary_flag.get() + 1));

    let seen = Rc::new(Cell::new(None));
    let seen_click = Rc::clone(&seen);
    let read = cx.clone();
    let secondary_wakes = Rc::new(Cell::new(0));
    let secondary_flag = Rc::clone(&secondary_wakes);
    let mut second = settled(
        &cx,
        div()
            .key("box")
            .style(
                StyleRefinement::default()
                    .width(90)
                    .height(20)
                    .flex_shrink(0.0),
            )
            .child(
                div()
                    .key("hit")
                    .style(StyleRefinement::default().width(20).height(20))
                    .on_click(move |_| {
                        seen_click.set(read.node_rect("box"));
                    })
                    .child(text("go")),
            ),
    );
    second.set_waker(move || secondary_flag.set(secondary_flag.get() + 1));
    click(&mut second, "hit");
    assert_eq!(seen.get().expect("click saw a rect").width, 90.0);
    assert_eq!(secondary_wakes.get(), 0);
    assert_eq!(primary_wakes.get(), 0);

    cx.set_scroll_offset("nowhere", Point::new(1.0, 1.0));
    assert_eq!(primary_wakes.get(), 1);
    assert_eq!(secondary_wakes.get(), 0);
}

#[test]
fn overlay_leaves_flow_escapes_clip_and_paints_above() {
    let cx = context();
    let ui = settled(
        &cx,
        div()
            .style(StyleRefinement::default().direction(FlexDirection::Column))
            .child(
                div()
                    .key("card")
                    .style(
                        StyleRefinement::default()
                            .direction(FlexDirection::Column)
                            .width(80)
                            .overflow(Overflow::Hidden)
                            .flex_shrink(0.0),
                    )
                    .children([
                        div().key("row").style(
                            StyleRefinement::default()
                                .width(80)
                                .height(30)
                                .flex_shrink(0.0)
                                .background(Color::BLACK),
                        ),
                        div()
                            .key("pop")
                            .overlay()
                            .place(Point::new(12.0, 90.0))
                            .style(
                                StyleRefinement::default()
                                    .width(40)
                                    .height(24)
                                    .flex_shrink(0.0)
                                    .background(Color::WHITE),
                            ),
                    ]),
            ),
    );
    let card = ui.node_rect(ui.find_by_key("card").unwrap()).unwrap();
    let pop = ui.node_rect(ui.find_by_key("pop").unwrap()).unwrap();
    assert!(
        (card.height - 30.0).abs() < 0.5,
        "card grew to {}",
        card.height
    );
    assert!((pop.x - 12.0).abs() < 0.5 && (pop.y - 90.0).abs() < 0.5);
    assert!(pop.bottom() > card.bottom());
    assert!(painted_quad(&ui, "pop").clip.is_none());
    assert!(quad_index(&ui, "pop") > quad_index(&ui, "row"));

    let pop_id = ui.find_by_key("pop").unwrap();
    let card_id = ui.find_by_key("card").unwrap();
    let on_pop = ui.hit_path(center_of(&ui, "pop"));
    assert!(on_pop.contains(&pop_id));
    assert!(!on_pop.contains(&card_id));
    let on_row = ui.hit_path(center_of(&ui, "row"));
    assert!(on_row.contains(&ui.find_by_key("row").unwrap()));
    assert!(!on_row.contains(&pop_id));
}

#[test]
fn wheel_over_an_overlay_does_not_scroll_the_page() {
    let cx = context();
    let start = Instant::now();
    let mut ui = Ui::new(
        &cx,
        div()
            .style(StyleRefinement::default().direction(FlexDirection::Column))
            .children([
                div()
                    .key("page")
                    .style(
                        StyleRefinement::default()
                            .direction(FlexDirection::Column)
                            .width(200)
                            .height(80)
                            .overflow_y(Overflow::Auto)
                            .flex_shrink(0.0),
                    )
                    .child(
                        div().key("tall").style(
                            StyleRefinement::default()
                                .width(40)
                                .height(400)
                                .flex_shrink(0.0),
                        ),
                    ),
                div()
                    .key("pop")
                    .overlay()
                    .place(Point::new(0.0, 0.0))
                    .style(
                        StyleRefinement::default()
                            .direction(FlexDirection::Column)
                            .width(60)
                            .height(40)
                            .overflow_y(Overflow::Auto)
                            .flex_shrink(0.0)
                            .background(Color::BLACK),
                    )
                    .child(
                        div().key("inner").style(
                            StyleRefinement::default()
                                .width(40)
                                .height(200)
                                .flex_shrink(0.0),
                        ),
                    ),
            ]),
        Theme::light(),
    );
    ui.update_at(VIEWPORT, start);
    let tall = ui.find_by_key("tall").unwrap();
    let inner = ui.find_by_key("inner").unwrap();
    let page_before = ui.node_rect(tall).unwrap().y;
    let inner_before = ui.node_rect(inner).unwrap().y;
    ui.handle_event(InputEvent::PointerMoved {
        position: center_of(&ui, "pop"),
    });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -40.0),
    });
    ui.update_at(VIEWPORT, start + Duration::from_millis(32));
    assert_eq!(ui.node_rect(tall).unwrap().y, page_before);
    assert!(ui.node_rect(inner).unwrap().y < inner_before);

    let page = ui.node_rect(ui.find_by_key("page").unwrap()).unwrap();
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(page.x + 150.0, page.y + 8.0),
    });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -80.0),
    });
    ui.update_at(VIEWPORT, start + Duration::from_millis(64));
    assert!(ui.node_rect(tall).unwrap().y < page_before);
}

#[test]
fn place_follows_the_trigger_rect() {
    let cx = context();
    let read = cx.clone();
    let mut ui = settled(
        &cx,
        div()
            .style(StyleRefinement::default().direction(FlexDirection::Column))
            .children([
                div()
                    .key("scroller")
                    .style(
                        StyleRefinement::default()
                            .direction(FlexDirection::Column)
                            .width(100)
                            .height(50)
                            .overflow_y(Overflow::Auto)
                            .flex_shrink(0.0),
                    )
                    .children([
                        div().key("trigger").style(
                            StyleRefinement::default()
                                .width(40)
                                .height(20)
                                .flex_shrink(0.0)
                                .background(Color::BLACK),
                        ),
                        div().style(
                            StyleRefinement::default()
                                .width(40)
                                .height(200)
                                .flex_shrink(0.0),
                        ),
                    ]),
                div()
                    .key("pop")
                    .overlay()
                    .place(Dynamic(move || {
                        read.node_rect("trigger")
                            .map(|rect| Point::new(rect.x, rect.bottom()))
                            .unwrap_or_default()
                    }))
                    .style(
                        StyleRefinement::default()
                            .width(30)
                            .height(16)
                            .flex_shrink(0.0)
                            .background(Color::WHITE),
                    ),
            ]),
    );
    let trigger = ui.node_rect(ui.find_by_key("trigger").unwrap()).unwrap();
    let pop = ui.node_rect(ui.find_by_key("pop").unwrap()).unwrap();
    assert!((pop.y - trigger.bottom()).abs() < 0.5, "pop.y {}", pop.y);
    assert!((pop.x - trigger.x).abs() < 0.5);

    cx.set_scroll_offset("scroller", Point::new(0.0, 30.0));
    ui.update(VIEWPORT);
    let trigger = ui.node_rect(ui.find_by_key("trigger").unwrap()).unwrap();
    let pop = ui.node_rect(ui.find_by_key("pop").unwrap()).unwrap();
    assert!(
        (pop.y - trigger.bottom()).abs() < 0.5,
        "pop {} trigger bottom {}",
        pop.y,
        trigger.bottom()
    );
    assert!(pop.y < 20.0);
}

#[test]
fn a_later_overlay_takes_the_hit_and_size_follows_content_or_the_viewport() {
    let cx = context();
    let back_clicks = cx.signal(0);
    let front_clicks = cx.signal(0);
    let back_count = back_clicks.clone();
    let front_count = front_clicks.clone();
    let ui = settled(
        &cx,
        div().children([
            div()
                .key("back")
                .overlay()
                .place(Point::new(10.0, 10.0))
                .style(
                    StyleRefinement::default()
                        .width(40)
                        .height(40)
                        .flex_shrink(0.0),
                )
                .on_click(move |_| back_count.update(|count| *count += 1)),
            div()
                .key("front")
                .overlay()
                .place(Point::new(10.0, 10.0))
                .style(
                    StyleRefinement::default()
                        .width(40)
                        .height(40)
                        .flex_shrink(0.0)
                        .background(Color::BLACK),
                )
                .on_click(move |_| front_count.update(|count| *count += 1)),
            div()
                .key("shrink")
                .overlay()
                .place(Point::new(0.0, 80.0))
                .child(
                    div().style(
                        StyleRefinement::default()
                            .width(30)
                            .height(20)
                            .flex_shrink(0.0),
                    ),
                ),
            div()
                .key("half")
                .overlay()
                .place(Point::new(0.0, 120.0))
                .style(
                    StyleRefinement::default()
                        .width(cappy_style::percent(50.0))
                        .height(10)
                        .flex_shrink(0.0),
                ),
        ]),
    );
    let path = ui.hit_path(center_of(&ui, "front"));
    assert!(path.contains(&ui.find_by_key("front").unwrap()));
    assert!(!path.contains(&ui.find_by_key("back").unwrap()));
    let mut ui = ui;
    click(&mut ui, "front");
    assert_eq!(front_clicks.get_untracked(), 1);
    assert_eq!(back_clicks.get_untracked(), 0);

    let shrunk = ui.node_rect(ui.find_by_key("shrink").unwrap()).unwrap();
    assert!((shrunk.width - 30.0).abs() < 0.5, "width {}", shrunk.width);
    assert!(
        (shrunk.height - 20.0).abs() < 0.5,
        "height {}",
        shrunk.height
    );
    let half = ui.node_rect(ui.find_by_key("half").unwrap()).unwrap();
    assert!(
        (half.width - 400.0).abs() < 0.5,
        "percent width {}",
        half.width
    );
}

#[test]
fn a_nested_overlay_hits_and_scrolls_above_its_host() {
    let cx = context();
    let start = Instant::now();
    let mut ui = Ui::new(
        &cx,
        div().child(
            div()
                .key("host")
                .overlay()
                .place(Point::new(0.0, 0.0))
                .style(
                    StyleRefinement::default()
                        .direction(FlexDirection::Column)
                        .width(120)
                        .height(80)
                        .overflow_y(Overflow::Auto)
                        .flex_shrink(0.0)
                        .background(Color::BLACK),
                )
                .children([
                    div().key("body").style(
                        StyleRefinement::default()
                            .width(40)
                            .height(200)
                            .flex_shrink(0.0),
                    ),
                    div()
                        .key("menu")
                        .overlay()
                        .place(Point::new(10.0, 10.0))
                        .style(
                            StyleRefinement::default()
                                .width(30)
                                .height(20)
                                .flex_shrink(0.0)
                                .background(Color::WHITE),
                        ),
                ]),
        ),
        Theme::light(),
    );
    ui.update_at(VIEWPORT, start);
    let path = ui.hit_path(Point::new(16.0, 16.0));
    assert!(path.contains(&ui.find_by_key("menu").unwrap()));
    assert!(!path.contains(&ui.find_by_key("host").unwrap()));
    assert!(quad_index(&ui, "menu") > quad_index(&ui, "host"));

    let body = ui.find_by_key("body").unwrap();
    let before = ui.node_rect(body).unwrap().y;
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(16.0, 16.0),
    });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -40.0),
    });
    ui.update_at(VIEWPORT, start + Duration::from_millis(32));
    assert_eq!(ui.node_rect(body).unwrap().y, before);

    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(80.0, 40.0),
    });
    ui.handle_event(InputEvent::Wheel {
        delta: Point::new(0.0, -80.0),
    });
    ui.update_at(VIEWPORT, start + Duration::from_millis(64));
    assert!(ui.node_rect(body).unwrap().y < before);
}

#[test]
fn a_hidden_overlay_is_not_hit() {
    let cx = context();
    let open = cx.signal(true);
    let shown = open.clone();
    let mut ui = settled(
        &cx,
        div().child(show(
            move || shown.get(),
            || {
                div()
                    .key("pop")
                    .overlay()
                    .place(Point::new(4.0, 4.0))
                    .style(
                        StyleRefinement::default()
                            .width(30)
                            .height(20)
                            .flex_shrink(0.0),
                    )
            },
        )),
    );
    let pop = ui.find_by_key("pop").unwrap();
    assert!(ui.hit_path(Point::new(10.0, 10.0)).contains(&pop));
    open.set(false);
    ui.update(VIEWPORT);
    assert!(!ui.hit_path(Point::new(10.0, 10.0)).contains(&pop));
}

#[test]
fn a_menu_that_closes_on_blur_still_runs_the_item_click() {
    let cx = context();
    let open = cx.signal(true);
    let open_menu = open.clone();
    let open_blur = open.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let click_log = Rc::clone(&log);
    let blur_log = Rc::clone(&log);
    let mut ui = settled(
        &cx,
        div().children([
            div()
                .key("trigger")
                .focusable()
                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                .on_focus(move |is_focused| {
                    if !is_focused {
                        blur_log.borrow_mut().push("blur");
                        open_blur.set(false);
                    }
                }),
            if_else(
                move || open_menu.get(),
                move || {
                    let click_log = Rc::clone(&click_log);
                    div()
                        .key("menu")
                        .overlay()
                        .place(Point::new(0.0, 48.0))
                        .child(
                            div()
                                .key("item")
                                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                                .on_click(move |_| {
                                    click_log.borrow_mut().push("click");
                                }),
                        )
                },
                || div(),
            ),
        ]),
    );
    key(&mut ui, Key::Tab, Modifiers::default());
    let item = center_of(&ui, "item");
    ui.handle_event(InputEvent::PointerMoved { position: item });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.update(VIEWPORT);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert!(ui.find_by_key("item").is_some());

    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(log.borrow().as_slice(), ["click", "blur"]);
    ui.update(VIEWPORT);
    assert!(ui.find_by_key("item").is_none());
    ui.prepare_frame();
    assert_eq!(ui.update(VIEWPORT), UpdateOutcome::Idle);
}

#[test]
fn a_focusable_item_keeps_its_click_when_the_trigger_blurs() {
    let cx = context();
    let open = cx.signal(true);
    let open_menu = open.clone();
    let open_blur = open.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let click_log = Rc::clone(&log);
    let blur_log = Rc::clone(&log);
    let mut ui = settled(
        &cx,
        div().children([
            div()
                .key("trigger")
                .focusable()
                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                .on_focus(move |is_focused| {
                    if !is_focused {
                        blur_log.borrow_mut().push("blur");
                        open_blur.set(false);
                    }
                }),
            if_else(
                move || open_menu.get(),
                move || {
                    let click_log = Rc::clone(&click_log);
                    div()
                        .key("menu")
                        .overlay()
                        .place(Point::new(0.0, 48.0))
                        .child(
                            div()
                                .key("item")
                                .focusable()
                                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                                .on_click(move |_| {
                                    click_log.borrow_mut().push("click");
                                }),
                        )
                },
                || div(),
            ),
        ]),
    );
    key(&mut ui, Key::Tab, Modifiers::default());
    let item = center_of(&ui, "item");
    ui.handle_event(InputEvent::PointerMoved { position: item });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.update(VIEWPORT);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert!(ui.find_by_key("item").is_some());
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(log.borrow().as_slice(), ["click", "blur"]);
}

#[test]
fn an_outside_click_dismisses_after_the_press_finishes() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    let page_log = Rc::clone(&log);
    let item_log = Rc::clone(&log);
    let dismiss_log = Rc::clone(&log);
    let mut ui = settled(
        &cx,
        div().children([
            div()
                .key("page")
                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                .on_click(move |_| page_log.borrow_mut().push("page")),
            div()
                .key("menu")
                .overlay()
                .place(Point::new(120.0, 120.0))
                .style(StyleRefinement::default().size(80).flex_shrink(0.0))
                .on_dismiss(move |_| dismiss_log.borrow_mut().push("dismiss"))
                .child(
                    div()
                        .key("item")
                        .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                        .on_click(move |_| item_log.borrow_mut().push("item")),
                ),
        ]),
    );
    let item = center_of(&ui, "item");
    ui.handle_event(InputEvent::PointerMoved { position: item });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.update(VIEWPORT);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(log.borrow().as_slice(), ["item"]);

    let page = center_of(&ui, "page");
    ui.handle_event(InputEvent::PointerMoved { position: page });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.update(VIEWPORT);
    assert_eq!(log.borrow().as_slice(), ["item"]);
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(log.borrow().as_slice(), ["item", "page", "dismiss"]);
}

#[test]
fn a_nested_overlay_click_does_not_dismiss_its_host() {
    let cx = context();
    let dismissals = cx.signal(0);
    let dismiss_flag = dismissals.clone();
    let nested_clicks = cx.signal(0);
    let nested_flag = nested_clicks.clone();
    let mut ui = settled(
        &cx,
        div()
            .key("host")
            .overlay()
            .place(Point::new(0.0, 0.0))
            .style(StyleRefinement::default().size(80).flex_shrink(0.0))
            .on_dismiss(move |_| dismiss_flag.update(|count| *count += 1))
            .child(
                div()
                    .key("nested")
                    .overlay()
                    .place(Point::new(200.0, 200.0))
                    .style(StyleRefinement::default().size(30).flex_shrink(0.0))
                    .on_click(move |_| nested_flag.update(|count| *count += 1)),
            ),
    );
    click(&mut ui, "host");
    click(&mut ui, "nested");
    assert_eq!(nested_clicks.get_untracked(), 1);
    assert_eq!(dismissals.get_untracked(), 0);

    click_at(&mut ui, 400.0, 400.0);
    assert_eq!(dismissals.get_untracked(), 1);
}

#[test]
fn the_click_that_opens_a_menu_does_not_dismiss_it() {
    let cx = context();
    let open = cx.signal(false);
    let open_click = open.clone();
    let open_menu = open.clone();
    let open_dismiss = open.clone();
    let dismissals = cx.signal(0);
    let dismiss_flag = dismissals.clone();
    let mut ui = settled(
        &cx,
        div().children([
            div()
                .key("trigger")
                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                .on_click(move |_| open_click.set(true)),
            if_else(
                move || open_menu.get(),
                move || {
                    let dismiss_flag = dismiss_flag.clone();
                    let open_dismiss = open_dismiss.clone();
                    div()
                        .key("menu")
                        .overlay()
                        .place(Point::new(0.0, 48.0))
                        .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                        .on_dismiss(move |_| {
                            dismiss_flag.update(|count| *count += 1);
                            open_dismiss.set(false);
                        })
                },
                || div(),
            ),
        ]),
    );
    click(&mut ui, "trigger");
    ui.update(VIEWPORT);
    assert!(ui.find_by_key("menu").is_some());
    assert_eq!(dismissals.get_untracked(), 0);

    click_at(&mut ui, 400.0, 400.0);
    ui.update(VIEWPORT);
    assert_eq!(dismissals.get_untracked(), 1);
    assert!(ui.find_by_key("menu").is_none());
}

#[test]
fn the_right_and_middle_buttons_reach_on_pointer_without_clicking_or_blurring() {
    let cx = context();
    let log = Rc::new(RefCell::new(Vec::new()));
    let events = Rc::clone(&log);
    let order = Rc::new(RefCell::new(Vec::new()));
    let click_order = Rc::clone(&order);
    let blur_order = Rc::clone(&order);
    let mut ui = settled(
        &cx,
        div().children([
            div()
                .key("keeper")
                .focusable()
                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                .on_focus(move |is_focused| {
                    if !is_focused {
                        blur_order.borrow_mut().push("blur");
                    }
                }),
            div()
                .key("target")
                .style(StyleRefinement::default().size(40).flex_shrink(0.0))
                .on_pointer(move |event| {
                    events.borrow_mut().push((event.button, event.phase));
                })
                .on_click(move |_| click_order.borrow_mut().push("click")),
        ]),
    );
    key(&mut ui, Key::Tab, Modifiers::default());
    let target = center_of(&ui, "target");
    for button in [PointerButton::Secondary, PointerButton::Middle] {
        log.borrow_mut().clear();
        ui.handle_event(InputEvent::PointerMoved { position: target });
        ui.handle_event(InputEvent::PointerButton {
            button,
            state: ButtonState::Pressed,
        });
        ui.handle_event(InputEvent::PointerButton {
            button,
            state: ButtonState::Released,
        });
        assert_eq!(
            log.borrow().as_slice(),
            [(button, PointerPhase::Down), (button, PointerPhase::Up)]
        );
    }
    assert!(order.borrow().is_empty(), "{:?}", order.borrow());

    log.borrow_mut().clear();
    ui.handle_event(InputEvent::PointerMoved { position: target });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerMoved {
        position: Point::new(target.x + 4.0, target.y),
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Primary,
        state: ButtonState::Released,
    });
    assert_eq!(
        log.borrow().as_slice(),
        [
            (PointerButton::Primary, PointerPhase::Down),
            (PointerButton::Primary, PointerPhase::Move),
            (PointerButton::Primary, PointerPhase::Up),
        ]
    );
    assert_eq!(order.borrow().as_slice(), ["click", "blur"]);
}

#[test]
fn a_disabled_node_does_not_take_the_right_button() {
    let cx = context();
    let events = Rc::new(Cell::new(0));
    let seen = events.clone();
    let mut ui = settled(
        &cx,
        div()
            .key("gate")
            .style(StyleRefinement::default().size(40).flex_shrink(0.0))
            .disabled(true)
            .on_pointer(move |_| seen.set(seen.get() + 1)),
    );
    let gate = center_of(&ui, "gate");
    ui.handle_event(InputEvent::PointerMoved { position: gate });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Secondary,
        state: ButtonState::Pressed,
    });
    ui.handle_event(InputEvent::PointerButton {
        button: PointerButton::Secondary,
        state: ButtonState::Released,
    });
    assert_eq!(events.get(), 0);
}

fn quad_index(ui: &Ui, key: &str) -> usize {
    let layout = ui.node_rect(ui.find_by_key(key).unwrap()).unwrap();
    let center_x = layout.x + layout.width * 0.5;
    let center_y = layout.y + layout.height * 0.5;
    ui.display_list()
        .iter()
        .position(|primitive| {
            let Primitive::Quad(quad) = primitive else {
                return false;
            };
            let x = quad.rect.x + quad.rect.width * 0.5;
            let y = quad.rect.y + quad.rect.height * 0.5;
            (x - center_x).abs() < 0.5 && (y - center_y).abs() < 0.5
        })
        .expect("painted quad")
}

fn sized_geometry(cx: &Context, width: i32) -> Element {
    div().children([
        div().key("box").style(
            StyleRefinement::default()
                .width(width)
                .height(20)
                .flex_shrink(0.0),
        ),
        rect_label(cx, "box").key("label"),
    ])
}
