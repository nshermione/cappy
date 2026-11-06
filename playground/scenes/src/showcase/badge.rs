//! One file for the badge: state, a parent prop, a click event and its sheet.

use cappy::prelude::*;

#[component]
pub struct Badge {
    hits: i64,
    #[prop]
    label: String,
    #[event]
    on_click: String,
}

stylesheet! {
    enum Class {
        Root {
            direction: FlexDirection::Row,
            align: Align::Center,
            padding_x: Space::Sm,
            padding_y: Space::Sm,
            radius: Radius::Full,
            background: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            font_size: FontSize::Sm,
        }
    }
}

impl Badge {
    fn press(&self) {
        self.hits.update(|hits| *hits += 1);
        self.on_click.emit(self.label.get());
    }
}

impl Component for Badge {
    fn stylesheet() -> &'static Stylesheet {
        Class::sheet()
    }

    fn render(self) -> Element {
        ui! {
            div(class = Class::Root, on_click = move |_| self.press()) {
                text(:format!("{} ({})", self.label, self.hits))
            }
        }
    }
}
