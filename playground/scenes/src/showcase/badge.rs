//! One file for the badge: state, a parent prop, a click event, two slots, its sheet, and a
//! demo that nests one badge in another's slot.

use std::sync::Arc;

use cappy::Class as ThemeClass;
use cappy::ClickEvent;
use cappy::prelude::*;

use crate::showcase::counter::Counter;

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

    fn render(&self, cx: &Context) -> Element {
        let counter = cx.use_store(Counter);
        ui! {
            div(class = Class::Root, on_click = cx.on(Self::press)) {
                slot(icon)
                text(:format!("{} ({}) c:{}", self.label, self.hits, counter.count), key = "badge-label")
                slot(tail)
            }
        }
    }
}

/// An outer badge whose icon slot is another badge. Each instance keeps its own `hits`.
pub fn nested(cx: &Context) -> Element {
    ui! {
        div(class = ThemeClass::Column) {
            text(
                "Each badge keeps its own hits. c: is the shared counter.",
                class = ThemeClass::Muted,
            )
            @Badge(label = "Outer", key = "outer-badge") {
                @Badge(label = "Inner", key = "inner-badge", slot = icon) {
                    text("•", key = "inner-mark", slot = icon)
                }
                text("end", key = "outer-tail", slot = tail)
            }
        }
    }
}

impl Badge {
    fn press(&self, cx: &Context, _event: &ClickEvent) {
        self.hits.update(|hits| *hits += 1);
        let counter = cx.use_store(Counter);
        counter.increment();
        self.on_click.emit(self.label.get());
    }
}
