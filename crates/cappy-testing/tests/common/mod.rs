//! Scenes and helpers shared by the GPU, CPU and parity pixel tests.
// Each test crate uses a different subset of these helpers.
#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use cappy_core::{
    Color, ColorToken, Damage, Element, FrameInput, Radius, RenderFrame, Signal, Theme, Ui,
};
use cappy_dsl::{style, ui};
use cappy_testing::TestUi;
use cappy_text::{CosmicTextMeasurer, FontContext};

pub const VIEWPORT: FrameInput = FrameInput {
    physical_width: 320,
    physical_height: 200,
    scale_factor: 1.0,
};

pub fn fonts() -> &'static FontContext {
    static FONTS: OnceLock<FontContext> = OnceLock::new();
    FONTS.get_or_init(FontContext::from_system)
}

pub fn harness(root: Element) -> TestUi {
    let mut ui = Ui::new(root, Theme::light());
    ui.set_text_measurer(CosmicTextMeasurer::new(fonts()));
    let mut harness = TestUi::from_ui(ui);
    harness.resize(VIEWPORT);
    harness
}

pub fn background() -> Color {
    Theme::light().tokens().colors.background
}

pub fn counter(count: Signal<i32>) -> Element {
    ui! {
        div(class = "app") {
            div(class = "row", key = "row") {
                text(move || format!("Count: {}", count.get()), key = "label")
                div(class = "btn btn-primary", key = "increment", on_click = move |_| count.update(|n| *n += 1)) {
                    text("+1")
                }
            }
        }
    }
}

pub fn shapes() -> Element {
    ui! {
        div(class = "app", key = "root") {
            div(class = "row") {
                div(style = style! { size: 60, background: ColorToken::Primary, radius: Radius::Lg })
                div(style = style! {
                    size: 60,
                    background: ColorToken::Surface,
                    border_color: ColorToken::Danger,
                    border_width: 3,
                    radius: Radius::Sm,
                })
                div(style = style! { size: 60, background: Color::hex("#22c55e") })
            }
        }
    }
}

pub fn full_repaint(frame: &RenderFrame) -> RenderFrame {
    RenderFrame {
        damage: Damage::Full,
        primitives: Arc::clone(&frame.primitives),
        ..frame.clone()
    }
}
