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
        element!((cx) {
            Container(class = [ThemeClass::Card, Class::Grow]) {
                Text("Counter", class = ThemeClass::Subtitle)
                Text(:format!("Count: {}", shared.count), key = "count", class = Class::Count)
                Text(:format!("Count: {}", mirror.count), key = "count-mirror", class = ThemeClass::Muted)
                Text(:format!("Local: {}", local.count), key = "count-local", class = ThemeClass::Muted)
                Container(class = ThemeClass::Row) {
                    Container(class = ThemeClass::Btn, key = "decrement", on_click = move |_| shared.decrement()) {
                        Text("-1")
                    }
                    Container(class = [ThemeClass::Btn, ThemeClass::BtnPrimary], key = "increment", on_click = move |_| shared.increment()) {
                        Text("+1")
                    }
                    Container(class = ThemeClass::Btn, key = "reset", on_click = move |_| shared.reset()) {
                        Text("Reset")
                    }
                    Container(class = ThemeClass::Btn, key = "local-increment", on_click = move |_| local.increment()) {
                        Text("Local +1")
                    }
                }
                Text("The badge shares this counter. Its hits stay on the badge.", class = ThemeClass::Muted)
                Badge(label = "Badge", key = "counter-badge") {
                    Text("•", key = "badge-mark", slot = icon)
                    Text("end", key = "badge-tail", slot = tail)
                }
            }
        })
    }
}
