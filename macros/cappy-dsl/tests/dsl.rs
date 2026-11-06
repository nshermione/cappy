use cappy_core::{
    Color, ColorToken, Component, Element, FrameInput, Radius, Space, StyleRefinement, Theme, Ui,
    div, signal, text,
};
use cappy_dsl::{style, theme, ui};

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 640,
    physical_height: 480,
    scale_factor: 1.0,
};

fn mounted(tree: Element) -> Ui {
    let mut ui = Ui::new(tree, Theme::light());
    ui.update(VIEWPORT);
    ui
}

fn text_of(ui: &Ui, key: &str) -> Option<String> {
    ui.node_text(ui.find_by_key(key)?).map(str::to_owned)
}

fn card() -> Element {
    div().class("card").child(text("card body"))
}

#[derive(Default)]
struct Badge {
    label: String,
}

impl Badge {
    fn label(mut self, label: &str) -> Self {
        self.label = label.to_owned();
        self
    }
}

impl Component for Badge {
    fn render(self) -> Element {
        div().class("badge").key("badge").child(text(self.label))
    }
}

#[test]
fn ui_builds_nested_nodes_with_attributes() {
    let ui = mounted(ui! {
        div(class = "app", key = "root") {
            text("Title", class = "title", key = "title")
            div(class = "row", key = "row") {
                text("a")
                text("b")
            }
        }
    });
    let root = ui.find_by_key("root").unwrap();
    assert_eq!(ui.node_children(root).len(), 2);
    assert_eq!(text_of(&ui, "title").as_deref(), Some("Title"));
    let row = ui.find_by_key("row").unwrap();
    assert_eq!(ui.node_children(row).len(), 2);
}

#[test]
fn closure_text_is_reactive() {
    let count = signal(1);
    let mut ui = mounted(ui! {
        div {
            text(:format!("Count: {}", count.get()), key = "label")
        }
    });
    count.set(2);
    ui.update(VIEWPORT);
    assert_eq!(text_of(&ui, "label").as_deref(), Some("Count: 2"));
}

#[test]
fn colon_fields_rebind_classes_and_styles() {
    let active = signal(false);
    let mut ui = mounted(ui! {
        div {
            div(
                key = "chip",
                class = "btn",
                :class = if active.get() { "btn-primary" } else { "" },
                :style = style! { width: if active.get() { 120 } else { 80 } },
            )
        }
    });
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
    assert_classes(&ui! { div(class = "btn") }, r#"["btn"]"#);
    assert_classes(&ui! { text("Title", class = "title") }, r#"["title"]"#);
}

#[test]
fn class_list_adds_every_item() {
    assert_classes(
        &ui! { div(class = ["btn", "btn-primary large"]) },
        r#"["btn", "btn-primary", "large"]"#,
    );
    assert_classes(
        &ui! { div(class = ["card", String::from("raised")], class = "wide") },
        r#"["card", "raised", "wide"]"#,
    );
    assert_classes(&ui! { div(class = []) }, "[]");
}

#[test]
fn fields_after_a_class_list_still_apply() {
    let ui = mounted(ui! {
        div {
            div(class = ["card"], key = "box")
            text("Title", class = ["title", "muted",], key = "title")
        }
    });
    assert!(ui.find_by_key("box").is_some());
    assert_eq!(text_of(&ui, "title").as_deref(), Some("Title"));
}

#[test]
fn colon_class_list_rebinds_every_item() {
    let active = signal(false);
    let mut ui = mounted(ui! {
        div {
            div(
                key = "chip",
                :class = ["btn", if active.get() { "btn-primary" } else { "" }],
            )
        }
    });
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
    let clicked = signal(false);
    let tree = ui! {
        div(key = "button", on_click = move |_| clicked.set(true), style = style! { size: 40 })
    };
    let ui = mounted(div().child(tree));
    assert!(ui.find_by_key("button").is_some());
}

#[test]
fn embedded_expressions_iterators_functions_and_components() {
    let items = ["one", "two", "three"];
    let ui = mounted(ui! {
        div(key = "root") {
            { text("embedded") }
            div(key = "list") {
                { ..items.iter().map(|item| text(*item)) }
            }
            card(key = "card")
            @Badge(label = "new")
        }
    });
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
    assert_classes(
        &ui! { @Badge(label = "new", class = ["wrap", "wide"]) },
        r#"["badge", "wrap", "wide"]"#,
    );
    let ui = mounted(ui! {
        @Badge(label = "new", class = "wrap", key = "hosted") {
            text("extra", key = "extra")
        }
    });
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
    let ui = mounted(ui! {
        div(key = "root") {
            label("hello", key = "label"),
            div,
            { text("embedded") },
        }
    });
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
fn theme_macro_overrides_tokens_and_classes() {
    let custom = theme! {
        name: "ocean",
        colors { primary: "#0ea5e9" },
        spacing { md: 10.0 },
        line_height: 1.5,
        classes { "pill" => style! { radius: Radius::Full } },
    };
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
fn theme_macro_extends_another_theme() {
    let dimmed = theme! {
        extends: Theme::dark(),
        radius { md: 2.0 },
    };
    assert_eq!(dimmed.name(), "dark");
    assert_eq!(dimmed.tokens().colors, Theme::dark().tokens().colors);
    assert_eq!(dimmed.tokens().radius.md, 2.0);
}
