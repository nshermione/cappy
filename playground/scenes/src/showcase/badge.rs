//! Badge: its own hits, a label prop, a click event, two slots, and its sheet.
//! The counter tab hosts one. Badges are not nested.

use std::sync::Arc;

use cappy::ClickEvent;
use cappy::prelude::*;

use super::store::Counter;

#[component]
pub struct Badge {
    hits: i64,
    #[prop]
    label: String,
    #[event]
    on_click: String,
    #[slots]
    icon: Element,
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

    fn render(&self) -> Element {
        let counter = use_store(Counter);
        element! {
            Container(class = Class::Root, on_click = on(Self::press)) {
                slot(icon)
                Text(:format!("{} ({}) c:{}", self.label, self.hits, counter.count), key = "badge-label")
                slot(tail)
            }
        }
    }
}

impl Badge {
    fn press(&self, _event: &ClickEvent) {
        self.hits.update(|hits| *hits += 1);
        let counter = use_store(Counter);
        counter.increment();
        self.on_click.emit(self.label.get());
    }
}
