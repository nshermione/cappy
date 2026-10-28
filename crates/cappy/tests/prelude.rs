use cappy::prelude::*;

#[derive(Clone, Copy, ClassName)]
enum Class {
    Card,
    Title,
}

#[derive(Default, Props)]
struct Greeting {
    name: String,
}

impl Component for Greeting {
    fn render(self) -> Element {
        ui! {
            div(class = Class::Card.as_str(), key = "greeting") {
                text(format!("Hello, {}!", self.name), class = Class::Title.as_str())
            }
        }
    }
}

#[test]
fn prelude_covers_a_small_app() {
    let accent = hex!("#ff8800");
    let custom = theme! {
        name: "warm",
        colors { primary: "#ff8800" },
    };
    let app = ui! {
        div(class = "app", style = style! { gap: Space::Lg, background: ColorToken::Background }) {
            @Greeting(name = "cappy")
        }
    };
    let mut ui = Ui::new(app, custom);
    assert!(ui.update(FrameInput::new(400, 300, 1.0)).needs_redraw());
    assert_eq!(ui.theme().tokens().colors.primary, accent);
    let greeting = ui.find_by_key("greeting").unwrap();
    let title = ui.node_children(greeting)[0];
    assert_eq!(ui.node_text(title), Some("Hello, cappy!"));
}
