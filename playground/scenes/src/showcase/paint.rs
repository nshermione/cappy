//! The paint tab: linear gradients, drop shadows and a fitted image.
//! Static looks live in the stylesheet. The lifted card reads a signal, so its shadow is inline.

use std::sync::Arc;

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::style::Class;

#[component]
pub struct PaintTab {
    lifted: bool,
}

impl Component for PaintTab {
    fn render(&self, _cx: &Context) -> Element {
        let pixels = landscape();
        let lifted = self.lifted;
        ui! {
            div(class = [ThemeClass::Card, Class::Grow]) {
                text("Paint", class = ThemeClass::Subtitle)
                text("Two-stop gradients, one drop shadow, and a fitted image.", class = ThemeClass::Muted)
                div(class = ThemeClass::Row) {
                    { specimen(Class::GradientRight, "Right", "gradient-right") }
                    { specimen(Class::GradientDown, "Down", "gradient-down") }
                    { specimen(Class::GradientLeft, "Left", "gradient-left") }
                }
                div(class = ThemeClass::Row) {
                    { specimen(Class::ShadowSoft, "Soft", "shadow-soft") }
                    { liftable(lifted) }
                    { specimen(Class::ShadowHard, "Hard", "shadow-hard") }
                    { specimen(Class::ShadowGlow, "Glow", "shadow-glow") }
                }
                div(class = ThemeClass::Row) {
                    { picture("Contain", ObjectFit::Contain, pixels.clone(), "picture-contain") }
                    { picture("Cover", ObjectFit::Cover, pixels.clone(), "picture-cover") }
                    { picture("Fill", ObjectFit::Fill, pixels, "picture-fill") }
                }
            }
        }
    }
}

fn specimen(class: Class, caption: &'static str, key: &'static str) -> Element {
    ui! {
        div(class = Class::Specimen) {
            div(class = class, key = key)
            text(caption, class = ThemeClass::Muted)
        }
    }
}

fn liftable(lifted: Reactive<bool>) -> Element {
    ui! {
        div(class = Class::Specimen) {
            div(
                class = Class::ShadowCard,
                key = "shadow-card",
                :style = style! {
                    shadow: shadow(
                        0,
                        if lifted.get() { 16 } else { 4 },
                        if lifted.get() { 24 } else { 8 },
                        0,
                        Color::rgba(15, 23, 42, 100),
                    )
                },
            ) {
                text(:if lifted.get() { "Lifted" } else { "Resting" }.to_owned(), key = "shadow-state")
            }
            div(
                class = ThemeClass::Btn,
                key = "shadow-toggle",
                on_click = move |_| lifted.update(|value| *value = !*value),
            ) {
                text("Lift")
            }
        }
    }
}

fn picture(
    caption: &'static str,
    fit: ObjectFit,
    pixels: Arc<ImagePixels>,
    key: &'static str,
) -> Element {
    ui! {
        div(class = Class::Specimen) {
            image(pixels, fit = fit, class = Class::Picture, key = key)
            text(caption, class = ThemeClass::Muted)
        }
    }
}

/// A wide landscape, so Contain letterboxes and Cover crops inside the square frame.
fn landscape() -> Arc<ImagePixels> {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 32;
    let mut rgba = vec![0; (WIDTH * HEIGHT * 4) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let index = ((y * WIDTH + x) * 4) as usize;
            let sun = (x as i32 - 14).pow(2) + (y as i32 - 10).pow(2) < 36;
            let [red, green, blue] = if sun {
                [251, 191, 36]
            } else if y < 18 {
                [56, 189, 248]
            } else {
                [34, 197, 94]
            };
            rgba[index..index + 4].copy_from_slice(&[red, green, blue, 255]);
        }
    }
    Arc::new(ImagePixels::from_rgba(WIDTH, HEIGHT, rgba).expect("landscape is 64 by 32 rgba"))
}
