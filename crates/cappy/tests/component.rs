use std::cell::Cell;
use std::rc::Rc;

use cappy::prelude::*;
use cappy::{ButtonState, Class as ThemeClass, InputEvent, PointerButton};

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
    fn press(&self) {
        self.on_click.emit(self.label.get());
    }
}

impl Component for Badge {
    fn stylesheet() -> &'static Stylesheet {
        BadgeClass::sheet()
    }

    fn render(self) -> Element {
        ui! {
            div(class = BadgeClass::Root, key = "badge-root", on_click = move |_| self.press()) {
                text(:format!("{}:{}", self.text, self.label), class = BadgeClass::Label, key = "badge-label")
            }
        }
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

fn settled(root: Element) -> Ui {
    let mut ui = Ui::new(root, Theme::light());
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
    let label = reactive(String::from("new"));
    let badge = Badge::default().label(label);
    badge.text.set(String::from("note"));
    let mut ui = settled(badge.render());
    let text = ui.find_by_key("badge-label").unwrap();
    assert_eq!(ui.node_text(text), Some("note:new"));

    badge.text.set(String::from("edited"));
    label.set(String::from("next"));
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(text), Some("edited:next"));
}

#[test]
fn owned_prop_does_not_follow_another_signal() {
    let badge = Badge::default().label("once");
    let mut ui = settled(badge.render());
    let text = ui.find_by_key("badge-label").unwrap();
    assert_eq!(ui.node_text(text), Some(":once"));

    reactive(String::from("other")).set(String::from("changed"));
    ui.update(VIEWPORT);
    assert_eq!(ui.node_text(text), Some(":once"));
}

#[test]
fn parent_passes_props_events_host_attributes_and_children() {
    let label = reactive(String::from("new"));
    let seen = Rc::new(Cell::new(String::new()));
    let record = Rc::clone(&seen);
    let mut ui = settled(ui! {
        @Badge(
            label = label,
            on_click = move |value: &String| record.set(value.clone()),
            class = ThemeClass::Muted,
            key = "hosted",
        ) {
            text("extra", key = "extra")
        }
    });

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
    let ping = Ping::default().on_ping(move || flag.set(true));
    ping.on_ping.emit(());
    assert!(hit.get());
    let _ = format!("{ping:?}");
}
