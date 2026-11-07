//! One file for the badge: state, a parent prop, a click event, two slots and its sheet.

use std::sync::Arc;

use cappy::prelude::*;
use cappy::ClickEvent;

#[component]
pub struct Badge {
    hits: i64,
    #[prop]
    label: String,
    #[event]
    on_click: String,
    #[slot]
    icon: Element,
    #[slot]
    tail: Element,
}

stylesheet! {
    enum Class {
        Root {
            direction: FlexDirection::Row,
            align: Align::Center,
            padding_x: Space::Sm,
            padding_y: Space::Sm,
            gap: Space::Sm,
            radius: Radius::Full,
            background: ColorToken::Primary,
            text_color: ColorToken::OnPrimary,
            font_size: FontSize::Sm,
        }
    }
}

impl Component for Badge {
    fn stylesheet() -> Option<Arc<Stylesheet>> {
        Some(Class::sheet())
    }

    fn render(&self, cx: &Context) -> Element {
        ui! {
            div(class = Class::Root, on_click = cx.on(Self::press)) {
                slot(icon)
                text(:format!("{} ({})", self.label, self.hits), key = "badge-label")
                slot(tail)
            }
        }
    }
}

impl Badge {
    fn press(&self, _event: &ClickEvent) {
        self.hits.update(|hits| *hits += 1);
        self.on_click.emit(self.label.get());
    }
}
