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
    fn render(&self) -> Element {
        let cx = Context::current();
        let shared = use_store(Counter);
        let mirror = use_store(Counter);
        let local = use_local_store(Counter);
        let name = reactive(String::new());
        let open_popup = {
            let cx = cx.clone();
            let shared = shared.clone();
            move |_: &cappy::ClickEvent| {
                let host = cx.clone();
                let shared = shared.clone();
                let popup_cx = host.clone();
                let popup_shared = shared.clone();
                host.open_surface(
                    SurfaceOptions::new("Counter").with_size(320.0, 200.0),
                    move |id| {
                        popup_cx.enter(|| popup_window(popup_cx.clone(), popup_shared.clone(), id))
                    },
                );
            }
        };
        element! {
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
                    div(class = ThemeClass::Btn, key = "popup", on_click = open_popup) {
                        text("Popup")
                    }
                }
                input(:value = name, placeholder = "Name", key = "name", class = Class::Field)
                text(:value = name, key = "name-echo", class = ThemeClass::Muted)
                text("The badge shares this counter. Its hits stay on the badge.", class = ThemeClass::Muted)
                Badge(label = "Badge", key = "counter-badge") {
                    text("•", key = "badge-mark", slot = icon)
                    text("end", key = "badge-tail", slot = tail)
                }
            }
        }
    }
}

fn popup_window(cx: Context, shared: Counter, id: SurfaceId) -> Element {
    // The root fills the window. A card radius would leave the window background in the corners.
    element! {
        div(class = ThemeClass::App) {
            text("Popup", class = ThemeClass::Subtitle)
            text(:format!("Count: {}", shared.count), key = "popup-count", class = ThemeClass::Muted)
            div(class = ThemeClass::Btn, key = "popup-close", on_click = move |_| cx.close_surface(id)) {
                text("Close")
            }
        }
    }
}
