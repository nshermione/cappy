//! The motion tab. Each button calls [`Context::animate`]; the shared driver eases one property.

use std::time::Duration;

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::style::Class;

#[component]
pub struct MotionTab {
    faded: bool,
    shifted: bool,
}

impl Component for MotionTab {
    fn render(&self) -> Element {
        let cx = Context::current();
        element! {
            Container(class = [ThemeClass::Card, Class::Grow]) {
                Text("Motion", class = ThemeClass::Subtitle)
                Text(
                    "Opacity and translation, started with Context::animate.",
                    class = ThemeClass::Muted,
                )
                Container(class = ThemeClass::Row) {
                    { opacity_card(cx.clone(), self.faded) }
                    { slide_card(cx, self.shifted) }
                }
            }
        }
    }
}

fn opacity_card(cx: Context, faded: Reactive<bool>) -> Element {
    element! {
        Container(class = Class::Specimen) {
            Container(class = Class::MotionOpacity, key = "motion-opacity") {
                Text("opacity")
            }
            Container(
                class = ThemeClass::Btn,
                key = "motion-fade",
                on_click = move |_| {
                    let next = !faded.get();
                    faded.set(next);
                    let opacity = if next { 0.2 } else { 1.0 };
                    cx.animate(move |ui| {
                        let id = ui.find_by_key("motion-opacity").expect("opacity card");
                        ui.animate(
                            id,
                            Motion::opacity(opacity)
                                .duration(Duration::from_millis(280))
                                .ease(Ease::OutCubic),
                        );
                    });
                },
            ) {
                Text("Fade")
            }
        }
    }
}

fn slide_card(cx: Context, shifted: Reactive<bool>) -> Element {
    element! {
        Container(class = Class::Specimen) {
            Container(class = Class::MotionSlide, key = "motion-slide") {
                Text("transform")
            }
            Container(
                class = ThemeClass::Btn,
                key = "motion-shift",
                on_click = move |_| {
                    let next = !shifted.get();
                    shifted.set(next);
                    let y = if next { 28.0 } else { 0.0 };
                    cx.animate(move |ui| {
                        let id = ui.find_by_key("motion-slide").expect("slide card");
                        ui.animate(
                            id,
                            Motion::translation(0.0, y)
                                .duration(Duration::from_millis(360))
                                .ease(Ease::OutBack),
                        );
                    });
                },
            ) {
                Text("Slide")
            }
        }
    }
}
