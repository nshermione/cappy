//! The counter tab. It reads the shared counter and hosts one badge.
//! That badge shares the counter and keeps its own hits.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::badge::Badge;
use super::store::Counter;
use super::style::Class;

#[component]
pub struct CounterTab {}

impl Component for CounterTab {
    fn render(&self, cx: &Context) -> Element {
        let shared = cx.use_store(Counter);
        let mirror = cx.use_store(Counter);
        let local = Counter::new(cx, 0);
        ui! {
            div(class = [ThemeClass::Card, Class::Grow]) {
                text("Counter", class = ThemeClass::Subtitle)
                text(:format!("Count: {}", shared.count), key = "count", class = Class::Count)
                text(:format!("Count: {}", mirror.count), key = "count-mirror", class = ThemeClass::Muted)
                text(:format!("Local: {}", local.count), key = "count-local", class = ThemeClass::Muted)
                div(class = ThemeClass::Row) {
                    div(class = ThemeClass::Btn, key = "decrement", on_click = move |_| shared.decrement()) {
                        text("-1")
                    }
                    div(class = [ThemeClass::Btn, ThemeClass::BtnPrimary], key = "increment", on_click = move |_| shared.increment()) {
                        text("+1")
                    }
                    div(class = ThemeClass::Btn, key = "reset", on_click = move |_| shared.reset()) {
                        text("Reset")
                    }
                    div(class = ThemeClass::Btn, key = "local-increment", on_click = move |_| local.increment()) {
                        text("Local +1")
                    }
                }
                text("The badge shares this counter. Its hits stay on the badge.", class = ThemeClass::Muted)
                @Badge(label = "Badge", key = "counter-badge") {
                    text("•", key = "badge-mark", slot = icon)
                    text("end", key = "badge-tail", slot = tail)
                }
            }
        }
    }
}
