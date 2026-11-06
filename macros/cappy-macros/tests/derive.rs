use cappy::{Reactive, reactive, ui};
use cappy_core::{Component, Element, FrameInput, Theme, Ui, div, text};
use cappy_macros::{ClassName, Props};

#[derive(Clone, Copy, ClassName)]
enum Class {
    Card,
    BtnPrimary,
    #[class(name = "title")]
    Heading,
}

/// A labelled card.
#[derive(Default, Props)]
struct InfoCard {
    /// Shown in bold at the top.
    title: String,
    subtitle: Option<String>,
    #[props(skip)]
    #[allow(dead_code)]
    internal_counter: u32,
}

impl Component for InfoCard {
    fn render(self) -> Element {
        let mut card = div()
            .class(Class::Card.as_str())
            .key("card")
            .child(text(self.title).class(Class::Heading.as_str()));
        if let Some(subtitle) = self.subtitle {
            card = card.child(text(subtitle));
        }
        card
    }
}

#[derive(Props)]
struct Generic<T: Clone> {
    value: T,
}

#[test]
fn props_setters_accept_conversions_and_wrap_options() {
    let card = InfoCard::default().title("Hello").subtitle("world");
    assert_eq!(card.title, "Hello");
    assert_eq!(card.subtitle.as_deref(), Some("world"));
}

#[test]
fn props_work_with_generic_structs() {
    let generic = Generic { value: 1_u8 }.value(7_u8);
    assert_eq!(generic.value, 7);
}

#[test]
fn derived_component_mounts_with_typed_classes() {
    let mut ui = Ui::new(
        div().child(InfoCard::default().title("Hello").subtitle("world")),
        Theme::light(),
    );
    ui.update(FrameInput::new(320, 240, 1.0));
    let card = ui.find_by_key("card").unwrap();
    assert_eq!(ui.node_children(card).len(), 2);
}

#[test]
fn class_names_are_kebab_case_with_overrides() {
    assert_eq!(Class::Card.as_str(), "card");
    assert_eq!(Class::BtnPrimary.as_str(), "btn-primary");
    assert_eq!(Class::Heading.to_string(), "title");
    assert_eq!(Class::ALL.len(), 3);
}

#[test]
fn every_class_exists_in_the_default_theme() {
    let theme = Theme::light();
    for class in Class::ALL {
        assert!(theme.has_class(class.as_ref()), "missing class {class}");
    }
}

#[test]
fn compile_errors_are_reported_clearly() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/*.rs");
}

#[reactive]
struct CounterState {
    count: i64,
}

#[reactive]
struct AppState {
    counter: CounterState,
    name: String,
}

#[reactive]
struct Row {
    count: i64,
    label: Reactive<String>,
}

#[test]
fn nested_reactive_fields_are_independent() {
    let state = AppState::new(CounterState::new(1), "a".into());
    state.counter.count.set(2);
    assert_eq!(state.counter.count.get(), 2);
    assert_eq!(state.name.get(), "a");
    assert_eq!(state.counter.count.to_string(), "2");
}

#[test]
fn already_reactive_field_keeps_its_handle() {
    let label = reactive(String::from("hi"));
    let row = Row::new(1, label);
    assert_eq!(row.label, label);
    label.set(String::from("yo"));
    assert_eq!(row.label.get(), "yo");
}

#[reactive]
struct Badge {
    label: String,
}

impl Component for Badge {
    fn render(self) -> Element {
        ui! {
            div(class = "badge", key = "badge") {
                text(:format!("{}", self.label))
            }
        }
    }
}

#[test]
fn reactive_component_label_updates_after_mount() {
    let badge = Badge::new(String::from("new"));
    let mut ui = Ui::new(badge.render(), Theme::light());
    let viewport = FrameInput::new(320, 240, 1.0);
    ui.update(viewport);
    let node = ui.find_by_key("badge").unwrap();
    let text_node = ui.node_children(node)[0];
    assert_eq!(ui.node_text(text_node), Some("new"));
    badge.label.set(String::from("next"));
    ui.update(viewport);
    assert_eq!(ui.node_text(text_node), Some("next"));
}
