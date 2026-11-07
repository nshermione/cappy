use cappy::prelude::*;

#[derive(Clone, Copy, ClassName)]
enum Class {
    Card,
    BtnPrimary,
    #[class(name = "title")]
    Heading,
}

/// A labelled card.
#[derive(Clone, Props)]
struct InfoCard {
    context: Context,
    /// Shown in bold at the top.
    title: String,
    subtitle: Option<String>,
    #[props(skip)]
    #[allow(dead_code)]
    internal_counter: u32,
}

impl HasContext for InfoCard {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for InfoCard {
    fn render(&self, _context: &Context) -> Element {
        let mut card = div()
            .class(Class::Card.as_str())
            .key("card")
            .child(text(self.title.clone()).class(Class::Heading.as_str()));
        if let Some(subtitle) = self.subtitle.clone() {
            card = card.child(text(subtitle));
        }
        card
    }
}

fn info_card(context: &Context) -> InfoCard {
    InfoCard {
        context: context.clone(),
        title: String::new(),
        subtitle: None,
        internal_counter: 0,
    }
}

#[derive(Props)]
struct Generic<T: Clone> {
    value: T,
}

#[test]
fn props_setters_accept_conversions_and_wrap_options() {
    let card = info_card(&Application::new().context().clone())
        .title("Hello")
        .subtitle("world");
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
    let context = Application::new().context().clone();
    let mut ui = Ui::new(
        &context,
        div().child(info_card(&context).title("Hello").subtitle("world")),
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

#[store]
struct CounterState {
    count: i64,
}

#[store]
struct AppState {
    counter: CounterState,
    name: String,
}

#[store]
struct Row {
    count: i64,
    label: Reactive<String>,
}

#[test]
fn nested_reactive_fields_are_independent() {
    let context = Application::new().context().clone();
    let state = AppState::new(&context, CounterState::new(&context, 1), "a".into());
    state.counter.count.set(2);
    assert_eq!(state.counter.count.get(), 2);
    assert_eq!(state.name.get(), "a");
    assert_eq!(state.counter.count.to_string(), "2");

    let shared = context.use_store(CounterState);
    shared.count.set(7);
    assert_eq!(context.use_store(CounterState).count.get(), 7);
    assert_eq!(state.counter.count.get(), 2);
}

#[test]
fn already_reactive_field_keeps_its_handle() {
    let context = Application::new().context().clone();
    let label = context.reactive(String::from("hi"));
    let row = Row::new(&context, 1, label.clone());
    assert_eq!(row.label, label);
    label.set(String::from("yo"));
    assert_eq!(row.label.get(), "yo");
}

#[component]
struct Badge {
    label: String,
}

impl Component for Badge {
    fn render(&self, _context: &Context) -> Element {
        ui! {
            div(class = "badge", key = "badge") {
                text(:format!("{}", self.label))
            }
        }
    }
}

#[store]
struct SharedCount {
    count: i64,
}

#[store]
struct SharedName {
    name: String,
}

#[store]
struct Pair<T> {
    value: T,
}

#[test]
fn singleton_is_shared_and_new_is_separate() {
    let context = Application::new().context().clone();
    let shared = context.use_store(SharedCount);
    shared.count.set(0);
    let again = context.use_store(SharedCount);
    shared.count.set(4);
    assert_eq!(again.count.get(), 4);

    let local = SharedCount::new(&context, 1);
    assert_eq!(local.count.get(), 1);
    local.count.set(9);
    assert_eq!(shared.count.get(), 4);

    let name = context.use_store(SharedName);
    name.name.set(String::from("a"));
    assert_eq!(context.use_store(SharedName).name.get(), "a");
    assert_eq!(SharedName::new(&context, String::from("b")).name.get(), "b");
    assert_eq!(shared.count.get(), 4);
    shared.count.set(0);

    let ints = context.use_store(Pair::<i64>());
    ints.value.set(3);
    assert_eq!(context.use_store(Pair::<i64>()).value.get(), 3);
    assert!(!context.use_store(Pair::<bool>()).value.get());
    assert_eq!(Pair::new(&context, 9_i64).value.get(), 9);
    assert_eq!(ints.value.get(), 3);
    ints.value.set(0);
}

#[derive(Clone, Debug, PartialEq, Eq, Reactive)]
struct Label(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reactive)]
enum Mode {
    #[default]
    One,
    Two,
}

#[derive(Copy)]
#[store]
struct Screen {
    tab: Mode,
}

#[test]
fn derive_reactive_wraps_a_plain_value() {
    let context = Application::new().context().clone();
    let label = context.reactive(Label(String::from("a")));
    assert_eq!(label.get(), Label(String::from("a")));
    label.set(Label(String::from("b")));
    assert_eq!(label.get(), Label(String::from("b")));

    let screen = context.use_store(Screen);
    assert_eq!(screen.tab.get(), Mode::One);
    screen.tab.set(Mode::Two);
    assert_eq!(context.use_store(Screen).tab.get(), Mode::Two);
}

#[test]
fn reactive_component_label_updates_after_mount() {
    let context = Application::new().context().clone();
    let badge = Badge::new(&context, String::from("new"));
    let mut ui = Ui::new(&context, badge.clone(), Theme::light());
    let viewport = FrameInput::new(320, 240, 1.0);
    ui.update(viewport);
    let node = ui.find_by_key("badge").unwrap();
    let text_node = ui.node_children(node)[0];
    assert_eq!(ui.node_text(text_node), Some("new"));
    badge.label.set(String::from("next"));
    ui.update(viewport);
    assert_eq!(ui.node_text(text_node), Some("next"));
}
