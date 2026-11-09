use cappy_core::{
    Application, Color, ColorToken, Component, Context, Element, FrameInput, HasContext, Radius,
    Space, StyleRefinement, Theme, Ui, div, text, text_dyn,
};
use cappy_dsl::{element, style};

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 640,
    physical_height: 480,
    scale_factor: 1.0,
};

fn context() -> Context {
    Application::new().context().clone()
}

fn mounted(context: &Context, tree: Element) -> Ui {
    let mut ui = Ui::new(context, tree, Theme::light());
    ui.update(VIEWPORT);
    ui
}

fn text_of(ui: &Ui, key: &str) -> Option<String> {
    ui.node_text(ui.find_by_key(key)?).map(str::to_owned)
}

fn card() -> Element {
    div().class("card").child(text("card body"))
}

#[derive(Clone)]
struct Badge {
    context: Context,
    label: String,
}

impl Badge {
    fn open(context: &Context) -> Self {
        Self {
            context: context.clone(),
            label: String::new(),
        }
    }

    fn label(mut self, label: &str) -> Self {
        self.label = label.to_owned();
        self
    }
}

impl HasContext for Badge {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for Badge {
    fn render(&self) -> Element {
        div()
            .class("badge")
            .key("badge")
            .child(text(self.label.clone()))
    }
}

#[test]
fn ui_builds_nested_nodes_with_attributes() {
    let cx = context();
    let ui = mounted(
        &cx,
        element!((cx) {
            div(class = "app", key = "root") {
                text("Title", class = "title", key = "title")
                div(class = "row", key = "row") {
                    text("a")
                    text("b")
                }
            }
        }),
    );
    let root = ui.find_by_key("root").unwrap();
    assert_eq!(ui.node_children(root).len(), 2);
    assert_eq!(text_of(&ui, "title").as_deref(), Some("Title"));
    let row = ui.find_by_key("row").unwrap();
    assert_eq!(ui.node_children(row).len(), 2);
}

#[test]
fn one_reactive_is_read_from_two_bindings_and_a_click() {
    let cx = context();
    let count = cx.reactive(0_i32);
    let mut ui = mounted(
        &cx,
        element!((cx) {
            div {
                text(:format!("{}", count.get()), key = "a")
                text(:format!("{}", count.get()), key = "b")
                div(key = "up", on_click = move |_| count.update(|value| *value += 1)) {
                    text("+")
                }
            }
        }),
    );
    assert_eq!(text_of(&ui, "a").as_deref(), Some("0"));
    assert_eq!(text_of(&ui, "b").as_deref(), Some("0"));
    count.set(2);
    ui.update(VIEWPORT);
    assert_eq!(text_of(&ui, "a").as_deref(), Some("2"));
    assert_eq!(text_of(&ui, "b").as_deref(), Some("2"));
}

#[test]
fn closure_text_is_reactive() {
    let cx = context();
    let count = cx.reactive(1_i32);
    let label = count.clone();
    let mut ui = mounted(
        &cx,
        element!((cx) {
            div {
                text(:format!("Count: {}", label.get()), key = "label")
            }
        }),
    );
    count.set(2);
    ui.update(VIEWPORT);
    assert_eq!(text_of(&ui, "label").as_deref(), Some("Count: 2"));
}

#[test]
fn colon_fields_rebind_classes_and_styles() {
    let cx = context();
    let active = cx.reactive(false);
    let active_class = active.clone();
    let active_style = active.clone();
    let mut ui = mounted(
        &cx,
        element!((cx) {
            div {
                div(
                    key = "chip",
                    class = "btn",
                    :class = if active_class.get() { "btn-primary" } else { "" },
                    :style = style! { width: if active_style.get() { 120 } else { 80 } },
                )
            }
        }),
    );
    let chip = ui.find_by_key("chip").unwrap();
    assert_eq!(ui.node_rect(chip).unwrap().width, 80.0);
    active.set(true);
    ui.update(VIEWPORT);
    assert_eq!(ui.node_rect(chip).unwrap().width, 120.0);
    assert_eq!(
        ui.node_style(chip).unwrap().visual.background,
        ui.theme().tokens().colors.primary
    );
}

fn assert_classes(element: &Element, expected: &str) {
    let debug = format!("{element:?}");
    assert!(
        debug.contains(&format!("classes: {expected}")),
        "expected classes {expected} in {debug}"
    );
}

#[test]
fn class_takes_one_value_without_brackets() {
    let cx = context();
    assert_classes(&element!((cx) { div(class = "btn") }), r#"["btn"]"#);
    assert_classes(
        &element!((cx) { text("Title", class = "title") }),
        r#"["title"]"#,
    );
}

#[test]
fn class_list_adds_every_item() {
    let cx = context();
    assert_classes(
        &element!((cx) { div(class = ["btn", "btn-primary large"]) }),
        r#"["btn", "btn-primary", "large"]"#,
    );
    assert_classes(
        &element!((cx) { div(class = ["card", String::from("raised")], class = "wide") }),
        r#"["card", "raised", "wide"]"#,
    );
    assert_classes(&element!((cx) { div(class = []) }), "[]");
}

#[test]
fn fields_after_a_class_list_still_apply() {
    let cx = context();
    let ui = mounted(
        &cx,
        element!((cx) {
            div {
                div(class = ["card"], key = "box")
                text("Title", class = ["title", "muted",], key = "title")
            }
        }),
    );
    assert!(ui.find_by_key("box").is_some());
    assert_eq!(text_of(&ui, "title").as_deref(), Some("Title"));
}

#[test]
fn colon_class_list_rebinds_every_item() {
    let cx = context();
    let active = cx.reactive(false);
    let active_class = active.clone();
    let mut ui = mounted(
        &cx,
        element!((cx) {
            div {
                div(
                    key = "chip",
                    :class = ["btn", if active_class.get() { "btn-primary" } else { "" }],
                )
            }
        }),
    );
    let chip = ui.find_by_key("chip").unwrap();
    let idle_background = ui.node_style(chip).unwrap().visual.background;
    assert_eq!(idle_background, ui.theme().tokens().colors.surface);
    active.set(true);
    ui.update(VIEWPORT);
    assert_eq!(
        ui.node_style(chip).unwrap().visual.background,
        ui.theme().tokens().colors.primary
    );
}

#[test]
fn handlers_pass_through_as_attributes() {
    let cx = context();
    let clicked = cx.reactive(false);
    let tree = element!((cx) {
        div(key = "button", on_click = move |_| clicked.set(true), style = style! { size: 40 })
    });
    let ui = mounted(&cx, div().child(tree));
    assert!(ui.find_by_key("button").is_some());
}

#[test]
fn embedded_expressions_iterators_functions_and_components() {
    let items = ["one", "two", "three"];
    let cx = context();
    let ui = mounted(
        &cx,
        element!((cx) {
            div(key = "root") {
                { text("embedded") }
                div(key = "list") {
                    { ..items.iter().map(|item| text(*item)) }
                }
                { card().key("card") }
                Badge(label = "new")
            }
        }),
    );
    let root = ui.find_by_key("root").unwrap();
    assert_eq!(ui.node_children(root).len(), 4);
    let list = ui.find_by_key("list").unwrap();
    assert_eq!(ui.node_children(list).len(), items.len());
    assert!(ui.find_by_key("card").is_some());
    let badge = ui.find_by_key("badge").unwrap();
    assert_eq!(ui.node_text(ui.node_children(badge)[0]), Some("new"));
}

#[test]
fn component_root_keeps_host_attributes_and_appended_children() {
    let cx = context();
    assert_classes(
        &element!((cx) { Badge(label = "new", class = ["wrap", "wide"]) }),
        r#"["badge", "wrap", "wide"]"#,
    );
    let cx = context();
    let ui = mounted(
        &cx,
        element!((cx) {
            Badge(label = "new", class = "wrap", key = "hosted") {
                text("extra", key = "extra")
            }
        }),
    );
    assert!(ui.find_by_key("badge").is_none());
    let hosted = ui.find_by_key("hosted").unwrap();
    assert_eq!(ui.node_children(hosted).len(), 2);
    assert_eq!(ui.node_text(ui.node_children(hosted)[0]), Some("new"));
    assert_eq!(text_of(&ui, "extra").as_deref(), Some("extra"));
}

fn label(content: &str) -> Element {
    text(content).class("label")
}

#[test]
fn any_element_function_takes_content_fields_and_comma_separated_children() {
    let cx = context();
    let ui = mounted(
        &cx,
        element!((cx) {
            div(key = "root") {
                { label("hello").key("label") },
                div,
                { text("embedded") },
            }
        }),
    );
    let root = ui.find_by_key("root").unwrap();
    assert_eq!(ui.node_children(root).len(), 3);
    assert_eq!(text_of(&ui, "label").as_deref(), Some("hello"));
}

#[test]
fn style_macro_matches_the_builder() {
    let from_macro = style! {
        padding: Space::Md,
        background: ColorToken::Surface,
        radius: Radius::Sm,
        width: 120,
    };
    let built = StyleRefinement::default()
        .padding(Space::Md)
        .background(ColorToken::Surface)
        .radius(Radius::Sm)
        .width(120);
    assert_eq!(from_macro, built);
}

#[test]
fn theme_overwrites_tokens_and_classes() {
    let mut custom = Theme::light();
    custom.set_name("ocean");
    custom.patch_tokens((
        ColorToken::patch().primary(Color::hex("#0ea5e9")),
        Space::patch().md(10.0),
    ));
    custom.tokens_mut().line_height = 1.5;
    custom.set_class("pill", style! { radius: Radius::Full });
    assert_eq!(custom.name(), "ocean");
    assert_eq!(custom.tokens().colors.primary, Color::hex("#0ea5e9"));
    assert_eq!(custom.tokens().spacing.md, 10.0);
    assert_eq!(custom.tokens().line_height, 1.5);
    assert!(custom.has_class("pill"));
    assert!(
        custom.has_class("btn"),
        "unspecified base keeps the light theme's classes"
    );
    assert_eq!(
        custom.tokens().colors.text,
        Theme::light().tokens().colors.text
    );
}

#[test]
fn control_flow_macros_mount_show_if_and_for() {
    #[derive(Clone, PartialEq)]
    struct Row {
        id: u32,
        title: String,
    }

    let cx = context();
    let open = cx.reactive(true);
    let ready = cx.reactive(false);
    let mode = cx.reactive(0_i32);
    let items = cx.reactive(vec![Row {
        id: 1,
        title: "row".into(),
    }]);
    let open_visible = open.clone();
    let open_text = open.clone();
    let ready_branch = ready.clone();
    let mode_branch = mode.clone();
    let items_source = items.clone();
    let mut ui = mounted(
        &cx,
        element!((cx) {
            div(key = "root") {
                show (:open_visible.get()) {
                    {{
                        let open_text = open_text.clone();
                        text_dyn(move || format!("open {}", open_text.get())).key("panel")
                    }}
                }
                if (:ready_branch.get()) {
                    text("yes", key = "state")
                } else {
                    {{
                        let mode_branch = mode_branch.clone();
                        text_dyn(move || {
                            if mode_branch.get() == 1 {
                                "mid".to_owned()
                            } else {
                                "no".to_owned()
                            }
                        })
                        .key("state")
                    }}
                }
                for (item in :items_source, key = item.id) {
                    text(:item.with(|row| row.title.clone()), key = "row")
                }
            }
        }),
    );
    assert_eq!(text_of(&ui, "panel").as_deref(), Some("open true"));
    assert_eq!(text_of(&ui, "state").as_deref(), Some("no"));
    assert_eq!(text_of(&ui, "row").as_deref(), Some("row"));

    mode.set(1);
    ui.update(VIEWPORT);
    assert_eq!(text_of(&ui, "state").as_deref(), Some("mid"));

    ready.set(true);
    ui.update(VIEWPORT);
    assert_eq!(text_of(&ui, "state").as_deref(), Some("yes"));

    open.set(false);
    ui.update(VIEWPORT);
    assert!(ui.find_by_key("panel").is_some());
    assert_eq!(text_of(&ui, "panel").as_deref(), Some("open false"));

    items.set(vec![Row {
        id: 1,
        title: "next".into(),
    }]);
    ui.update(VIEWPORT);
    assert_eq!(text_of(&ui, "row").as_deref(), Some("next"));
}

#[derive(Clone)]
struct Mark {
    context: Context,
}

impl Mark {
    fn open(context: &Context) -> Self {
        Self {
            context: context.clone(),
        }
    }
}

impl HasContext for Mark {
    fn context(&self) -> &Context {
        &self.context
    }
}

impl Component for Mark {
    fn render(&self) -> Element {
        text("mark")
    }
}

fn marked(cx: &Context) -> Element {
    let value = cx.reactive(String::from("helper"));
    element!((cx) {
        text(:value.get(), key = "helper")
    })
}

/// `show` / `if` / `for` build elements later. `cx` is only borrowed for the call, and each block
/// still receives it.
#[test]
fn deferred_blocks_receive_the_callers_context() {
    fn tree(context: &Context) -> Element {
        let shown = context.reactive(true);
        let first = context.reactive(false);
        let second = context.reactive(true);
        let rows = context.reactive(vec!["row".to_owned()]);
        element!((context) {
            div(key = "root") {
                show (:shown.get()) {
                    Mark(key = "mark-show")
                }
                show (:shown.get()) {
                    Mark(key = "mark-show-again")
                }
                if (:first.get()) {
                    Mark(key = "mark-then")
                } else if (:second.get()) {
                    Mark(key = "mark-else-if")
                } else {
                    Mark(key = "mark-else")
                }
                show (:shown.get()) {
                    { marked(context) }
                }
                for (item in :rows, key = item) {
                    Mark(key = "mark-row")
                    text(:item.get(), key = "row-text")
                }
            }
        })
    }

    let cx = context();
    let ui = mounted(&cx, tree(&cx));
    assert_eq!(text_of(&ui, "mark-show").as_deref(), Some("mark"));
    assert_eq!(text_of(&ui, "mark-show-again").as_deref(), Some("mark"));
    assert!(ui.find_by_key("mark-then").is_none());
    assert_eq!(text_of(&ui, "mark-else-if").as_deref(), Some("mark"));
    assert!(ui.find_by_key("mark-else").is_none());
    assert_eq!(text_of(&ui, "helper").as_deref(), Some("helper"));
    assert_eq!(text_of(&ui, "mark-row").as_deref(), Some("mark"));
    assert_eq!(text_of(&ui, "row-text").as_deref(), Some("row"));
}

#[test]
fn overwrites_extend_another_theme() {
    let mut dimmed = Theme::dark();
    dimmed.patch_tokens(Radius::patch().md(2.0));
    assert_eq!(dimmed.name(), "dark");
    assert_eq!(dimmed.tokens().colors, Theme::dark().tokens().colors);
    assert_eq!(dimmed.tokens().radius.md, 2.0);
}
