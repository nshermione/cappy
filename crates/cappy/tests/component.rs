use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use cappy::prelude::*;
use cappy::{ButtonState, Class as ThemeClass, ClickEvent, InputEvent, PointerButton};

#[component]
struct Badge {
    text: String,
    /// Parent-owned label.
    #[prop]
    label: String,
    #[event]
    on_click: String,
}

stylesheet! {
    enum BadgeClass {
        Root {
            padding: Space::Sm,
            background: ColorToken::Primary,
        }
        Label {
            background: ColorToken::Success,
        }
    }
}

impl Badge {
    fn press(&self, _event: &ClickEvent) {
        self.on_click.emit(self.label.get());
    }
}

impl Component for Badge {
    fn stylesheet() -> Option<Arc<Stylesheet>> {
        Some(BadgeClass::sheet())
    }

    fn render(&self) -> Element {
        let cx = Context::current();
        element!((cx) {
            div(class = BadgeClass::Root, key = "badge-root", on_click = cx.on(Self::press)) {
                text(:format!("{}:{}", self.text, self.label), class = BadgeClass::Label, key = "badge-label")
            }
        })
    }
}

#[derive(Debug)]
#[component]
struct Ping {
    #[event]
    on_ping: (),
}

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 320,
    physical_height: 240,
    scale_factor: 1.0,
};

fn context() -> Context {
    Application::new().context().clone()
}

fn settled(cx: &Context, root: Element) -> Ui {
    let mut ui = Ui::new(cx, root, Theme::light());
    ui.update(VIEWPORT);
    ui.prepare_frame();
    ui
}

fn click(ui: &mut Ui, key: &str) {
    let rect = ui.node_rect(ui.find_by_key(key).unwrap()).unwrap();
    let position = cappy::Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    ui.handle_event(InputEvent::PointerMoved { position });
    for state in [ButtonState::Pressed, ButtonState::Released] {
        ui.handle_event(InputEvent::PointerButton {
            button: PointerButton::Primary,
            state,
        });
    }
}

#[test]
fn plain_fields_and_shared_props_update_rendered_text() {
    let cx = context();
    let label = cx.reactive(String::from("new"));
    let badge = Badge::open(&cx).label(label.clone());
    badge.text.set(String::from("note"));
    let mut ui = settled(&cx, badge.clone().into_element());
    let text = ui.find_by_key("badge-label").unwrap();
    assert_eq!(ui.node_text(text), Some("note:new"));

    badge.text.set(String::from("edited"));
    label.set(String::from("next"));
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(text), Some("edited:next"));
}

#[test]
fn owned_prop_does_not_follow_another_signal() {
    let cx = context();
    let badge = Badge::open(&cx).label("once");
    let mut ui = settled(&cx, badge.into_element());
    let text = ui.find_by_key("badge-label").unwrap();
    assert_eq!(ui.node_text(text), Some(":once"));

    cx.reactive(String::from("other"))
        .set(String::from("changed"));
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(text), Some(":once"));
}

#[test]
fn parent_passes_props_events_host_attributes_and_children() {
    let cx = context();
    let label = cx.reactive(String::from("new"));
    let seen = Rc::new(Cell::new(String::new()));
    let record = Rc::clone(&seen);
    let mut ui = settled(
        &cx,
        element!((cx) {
            Badge(
                label = label.clone(),
                on_click = move |value: &String| record.set(value.clone()),
                class = ThemeClass::Muted,
                key = "hosted",
            ) {
                text("extra", key = "extra"),
                text("more", key = "more"),
            }
        }),
    );

    assert!(ui.find_by_key("badge-root").is_none());
    let hosted = ui.find_by_key("hosted").unwrap();
    let hosted_style = ui.node_style(hosted).unwrap();
    assert_eq!(
        hosted_style.visual.background,
        ui.theme().tokens().colors.primary
    );
    assert_eq!(
        hosted_style.text.color,
        ui.theme().tokens().colors.text_muted
    );
    let label_node = ui.find_by_key("badge-label").unwrap();
    assert_eq!(
        ui.node_style(label_node).unwrap().visual.background,
        ui.theme().tokens().colors.success
    );
    assert!(!ui.theme().has_class("label"));
    assert_eq!(ui.node_text(label_node), Some(":new"));
    assert_eq!(
        ui.node_text(ui.find_by_key("extra").unwrap()),
        Some("extra")
    );
    assert_eq!(ui.node_text(ui.find_by_key("more").unwrap()), Some("more"));

    click(&mut ui, "hosted");
    assert_eq!(seen.take(), "new");
    label.set(String::from("next"));
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(label_node), Some(":next"));
    click(&mut ui, "hosted");
    assert_eq!(seen.take(), "next");
}

#[test]
fn unit_event_emits_without_a_payload() {
    let hit = Rc::new(Cell::new(false));
    let flag = Rc::clone(&hit);
    let ping = Ping::open(&context()).on_ping(move || flag.set(true));
    ping.on_ping.emit(());
    assert!(hit.get());
    let _ = format!("{ping:?}");
}

#[component]
struct Panel {
    #[slot]
    body: Element,
    #[slot]
    icon: Element,
}

impl Component for Panel {
    fn render(&self) -> Element {
        let cx = Context::current();
        element!((cx) {
            div(key = "panel") {
                slot(icon)
                slot(body)
            }
        })
    }
}

#[component]
struct Frame {
    #[slot]
    body: Element,
}

impl Component for Frame {
    fn render(&self) -> Element {
        let cx = Context::current();
        element!((cx) {
            div(key = "frame") {
                text("before", key = "before")
                slot(body)
                text("after", key = "after")
            }
        })
    }
}

#[component]
struct Inner {
    #[slot]
    body: Element,
}

impl Component for Inner {
    fn render(&self) -> Element {
        let cx = Context::current();
        element!((cx) {
            div(key = "inner") { slot(body) }
        })
    }
}

#[component]
struct Outer {
    #[slot]
    body: Element,
}

impl Component for Outer {
    fn render(&self) -> Element {
        let cx = Context::current();
        element!((cx) {
            div(key = "outer") {
                slot(body)
                Inner { text("inner-text", key = "inner-text") }
            }
        })
    }
}

#[test]
fn slots_land_in_declaration_order_and_drop_the_rest() {
    let cx = context();
    let panel = Panel::open(&cx);
    let _rendered = cx.enter(|| panel.render());
    let _again = panel.clone();
    let ui = settled(
        &cx,
        element!((cx) {
            Panel(class = ThemeClass::Muted, key = "hosted") {
                text("default", key = "default"),
                text("named", key = "named", slot = body),
                div(slot = icon, key = "icon") { text("+", key = "plus") }
                text("extra", key = "extra"),
            }
        }),
    );

    assert!(ui.find_by_key("panel").is_none());
    assert!(ui.find_by_key("named").is_none());
    assert!(ui.find_by_key("extra").is_none());
    let hosted = ui.find_by_key("hosted").unwrap();
    assert_eq!(
        ui.node_style(hosted).unwrap().text.color,
        ui.theme().tokens().colors.text_muted
    );
    let children = ui.node_children(hosted);
    assert_eq!(children.len(), 2);
    assert_eq!(ui.node_text(ui.node_children(children[0])[0]), Some("+"));
    assert_eq!(ui.node_text(children[1]), Some("default"));
}

#[test]
fn an_unnamed_child_is_placed_where_the_slot_is() {
    let cx = context();
    let ui = settled(
        &cx,
        element!((cx) {
            Frame {
                text("middle", key = "middle"),
                text("dropped", key = "dropped"),
            }
        }),
    );
    let frame = ui.find_by_key("frame").unwrap();
    let children = ui.node_children(frame);
    assert_eq!(children.len(), 3);
    assert_eq!(ui.node_text(children[0]), Some("before"));
    assert_eq!(ui.node_text(children[1]), Some("middle"));
    assert_eq!(ui.node_text(children[2]), Some("after"));
    assert!(ui.find_by_key("dropped").is_none());
}

#[test]
fn nested_components_keep_their_own_slots() {
    let cx = context();
    let ui = settled(
        &cx,
        element!((cx) {
            Outer {
                text("outer-text", key = "outer-text")
            }
        }),
    );
    let outer = ui.find_by_key("outer").unwrap();
    let outer_children = ui.node_children(outer);
    assert_eq!(ui.node_text(outer_children[0]), Some("outer-text"));
    let inner = ui.find_by_key("inner").unwrap();
    assert_eq!(ui.node_text(ui.node_children(inner)[0]), Some("inner-text"));
}

#[component]
struct Shelf {
    open: bool,
    #[props]
    title: String,
    #[event]
    on_open: (),
    subtitle: String,
    #[events]
    on_change: String,
    on_close: (),
    #[slots]
    header: Element,
    body: Element,
    #[state]
    query: String,
}

impl Component for Shelf {
    fn render(&self) -> Element {
        let cx = Context::current();
        element!((cx) {
            div(key = "shelf") {
                slot(header)
                slot(body)
            }
        })
    }
}

#[test]
fn section_markers_cover_following_fields_until_the_next_marker() {
    let cx = context();
    let opened = Rc::new(Cell::new(false));
    let changed = Rc::new(Cell::new(String::new()));
    let closed = Rc::new(Cell::new(false));
    let opened_flag = Rc::clone(&opened);
    let changed_flag = Rc::clone(&changed);
    let closed_flag = Rc::clone(&closed);
    let shelf = Shelf::open(&cx)
        .title("A")
        .on_open(move || opened_flag.set(true))
        .subtitle("B")
        .on_change(move |value: &String| changed_flag.set(value.clone()))
        .on_close(move || closed_flag.set(true));
    shelf.open.set(true);
    shelf.query.set(String::from("q"));

    assert!(shelf.open.get());
    assert_eq!(shelf.title.get(), "A");
    assert_eq!(shelf.subtitle.get(), "B");
    assert_eq!(shelf.query.get(), "q");
    shelf.on_open.emit(());
    shelf.on_change.emit(String::from("next"));
    shelf.on_close.emit(());
    assert!(opened.get());
    assert_eq!(changed.take(), "next");
    assert!(closed.get());

    let ui = settled(
        &cx,
        element!((cx) {
            Shelf(title = "A", subtitle = "B", key = "hosted") {
                text("top", key = "top", slot = header),
                text("main", key = "main", slot = body),
                text("extra", key = "extra"),
            }
        }),
    );
    assert!(ui.find_by_key("shelf").is_none());
    assert!(ui.find_by_key("extra").is_none());
    let hosted = ui.find_by_key("hosted").unwrap();
    let children = ui.node_children(hosted);
    assert_eq!(children.len(), 2);
    assert_eq!(ui.node_text(children[0]), Some("top"));
    assert_eq!(ui.node_text(children[1]), Some("main"));
}
