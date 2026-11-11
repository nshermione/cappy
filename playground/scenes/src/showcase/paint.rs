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
    fn render(&self) -> Element {
        let pixels = landscape();
        let lifted = self.lifted;
        element! {
            div(class = [ThemeClass::Card, Class::Grow]) {
                text("Paint", class = ThemeClass::Subtitle)
                text("Two-stop gradients, one drop shadow, and a fitted image.", class = ThemeClass::Muted)
                div(class = ThemeClass::Row) {
                    { specimen(Class::GradientRight, "Right", "gradient-right") }
                    { specimen(Class::GradientDown, "Down", "gradient-down") }
                    { specimen(Class::GradientLeft, "Left", "gradient-left") }
                }
                { style_gaps() }
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

/// One row of the style that used to be missing: an uneven border, a spanned grid cell,
/// and a header that sticks inside a short scroller.
fn style_gaps() -> Element {
    element! {
        div(class = ThemeClass::Row) {
            div(key = "edge-card", style = style! {
                width: 88,
                height: 56,
                background: ColorToken::Surface,
                border_color: ColorToken::Primary,
                border_top: 6,
                radius_top_left: 18,
                opacity: 0.7,
            })
            div(style = style! {
                display: Display::Grid,
                grid_columns: 3,
                width: 96,
                height: 56,
                gap: 0,
            }) {
                div(key = "span-wide", style = style! {
                    column_span: 2,
                    height: 28,
                    background: ColorToken::Primary,
                })
                div(style = style! { height: 28, background: ColorToken::Success })
                div(style = style! { height: 28, background: ColorToken::Warning })
            }
            div(key = "sticky-port", style = style! {
                width: 120,
                height: 72,
                overflow_y: Overflow::Auto,
                border_width: 1,
                border_color: ColorToken::Border,
                direction: FlexDirection::Column,
            }) {
                div(key = "sticky-head", style = style! {
                    position: Position::Sticky,
                    top: 0,
                    height: 22,
                    flex_shrink: 0.0,
                    background: ColorToken::Surface,
                    cursor: CursorIcon::Grab,
                    align: Align::Center,
                    padding_x: 8,
                }) {
                    text("Sticky")
                }
                div(style = style! { height: 48, flex_shrink: 0.0, background: ColorToken::Primary, opacity: 0.45 })
                div(style = style! { height: 48, flex_shrink: 0.0, background: ColorToken::Success, opacity: 0.45 })
            }
        }
    }
}

fn specimen(class: Class, caption: &'static str, key: &'static str) -> Element {
    element! {
        div(class = Class::Specimen) {
            div(class = class, key = key)
            text(caption, class = ThemeClass::Muted)
        }
    }
}

fn liftable(lifted: Reactive<bool>) -> Element {
    element! {
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
    element! {
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
