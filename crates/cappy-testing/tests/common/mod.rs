//! Scenes and helpers shared by the GPU, CPU and parity pixel tests.
// Each test crate uses a different subset of these helpers.
#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use cappy_core::{
    Application, Color, ColorToken, Context, Damage, Element, FrameInput, ImagePixels, ObjectFit,
    Radius, Reactive, RenderFrame, Theme, Ui, linear, shadow,
};
use cappy_dsl::{element, style};
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

pub fn context() -> Context {
    Application::new().context().clone()
}

pub fn harness(context: &Context, root: Element) -> TestUi {
    let mut ui = Ui::new(context, root, Theme::light());
    ui.set_text_measurer(CosmicTextMeasurer::new(fonts()));
    let mut harness = TestUi::from_ui(ui);
    harness.resize(VIEWPORT);
    harness
}

pub fn background() -> Color {
    Theme::light().tokens().colors.background
}

pub fn counter(cx: &Context, count: Reactive<i32>) -> Element {
    let label = count.clone();
    element!((cx) {
        div(class = "app") {
            div(class = "row", key = "row") {
                text(:format!("Count: {}", label.get()), key = "label")
                div(class = "btn btn-primary", key = "increment", on_click = move |_| count.update(|n| *n += 1)) {
                    text("+1")
                }
            }
        }
    })
}

pub fn shapes(cx: &Context) -> Element {
    element!((cx) {
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
    })
}

pub fn effects(cx: &Context) -> Element {
    let pixels = Arc::new(
        ImagePixels::from_rgba(
            2,
            2,
            vec![
                239, 68, 68, 255, 59, 130, 246, 255, 59, 130, 246, 255, 239, 68, 68, 255,
            ],
        )
        .expect("2x2 rgba"),
    );
    element!((cx) {
        div(style = style! { padding: 24, gap: 16 }) {
            div(key = "gradient", style = style! {
                size: 64,
                background: linear(Color::hex("#ef4444"), Color::hex("#3b82f6")).to_right(),
                radius: 8,
            })
            div(key = "shadow", style = style! {
                size: 64,
                background: Color::hex("#f8fafc"),
                radius: 12,
                shadow: shadow(0, 8, 16, 0, Color::rgba(15, 23, 42, 90)),
            })
            image(
                pixels,
                key = "photo",
                fit = ObjectFit::Contain,
                style = style! { size: 64, radius: 16, background: Color::hex("#e4e4e7") },
            )
        }
    })
}

pub fn full_repaint(frame: &RenderFrame) -> RenderFrame {
    RenderFrame {
        damage: Damage::Full,
        primitives: Arc::clone(&frame.primitives),
        ..frame.clone()
    }
}
