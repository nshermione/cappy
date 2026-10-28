//! Pixel tests on a real or software GPU. Run with `cargo test -p cappy-testing --features gpu-tests`.
#![cfg(feature = "gpu-tests")]

use std::path::Path;
use std::sync::{Arc, OnceLock};

use cappy_core::{
    Color, ColorToken, Damage, Element, FrameInput, Radius, RenderFrame, Signal, Theme, Ui, signal,
};
use cappy_dsl::{style, ui};
use cappy_render::{CosmicTextMeasurer, FontContext};
use cappy_testing::TestUi;
use cappy_testing::gpu::{HeadlessRenderer, Image, assert_snapshot};

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 320,
    physical_height: 200,
    scale_factor: 1.0,
};
/// Channel difference allowed between GPUs for anti-aliased edges.
const SNAPSHOT_TOLERANCE: u8 = 2;

fn fonts() -> &'static FontContext {
    static FONTS: OnceLock<FontContext> = OnceLock::new();
    FONTS.get_or_init(FontContext::from_system)
}

fn renderer() -> HeadlessRenderer {
    HeadlessRenderer::new(fonts()).expect("headless GPU available")
}

fn harness(root: Element) -> TestUi {
    let mut ui = Ui::new(root, Theme::light());
    ui.set_text_measurer(CosmicTextMeasurer::new(fonts()));
    let mut harness = TestUi::from_ui(ui);
    harness.resize(VIEWPORT);
    harness
}

fn background() -> Color {
    Theme::light().tokens().colors.background
}

fn counter(count: Signal<i32>) -> Element {
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

fn shapes() -> Element {
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

fn full_repaint(frame: &RenderFrame) -> RenderFrame {
    RenderFrame {
        damage: Damage::Full,
        primitives: Arc::clone(&frame.primitives),
        ..frame.clone()
    }
}

fn probe(image: &Image, harness: &TestUi, key: &str) -> [u8; 4] {
    let center = harness.center(key);
    image.pixel(center.x as u32, center.y as u32)
}

#[test]
fn shapes_match_snapshot() {
    let mut harness = harness(shapes());
    let image = renderer().render(&harness.frame(), background()).unwrap();
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    assert_snapshot(&snapshots, "shapes", &image, SNAPSHOT_TOLERANCE);
}

#[test]
fn colors_land_where_layout_put_them() {
    let mut harness = harness(counter(signal(0)));
    let image = renderer().render(&harness.frame(), background()).unwrap();
    let colors = Theme::light().tokens().colors.clone();
    assert_eq!(image.pixel(1, 1), colors.background.to_rgba8());
    let button_edge = harness.rect("increment");
    let inside_button = image.pixel(
        button_edge.x as u32 + 3,
        (button_edge.y + button_edge.height / 2.0) as u32,
    );
    assert_eq!(inside_button, colors.primary.to_rgba8());
}

#[test]
fn text_is_drawn_inside_its_rect() {
    let mut harness = harness(counter(signal(0)));
    let image = renderer().render(&harness.frame(), background()).unwrap();
    let label = harness.rect("label");
    let background = background().to_rgba8();
    let inked = (label.x as u32..label.right() as u32)
        .flat_map(|x| (label.y as u32..label.bottom() as u32).map(move |y| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y) != background)
        .count();
    assert!(
        inked > 20,
        "expected glyph pixels inside the label, found {inked}"
    );
}

#[test]
fn partial_repaint_matches_a_full_repaint() {
    let count = signal(0);
    let mut harness = harness(counter(count));
    let mut incremental = renderer();
    incremental.render(&harness.frame(), background()).unwrap();

    harness.click("increment");
    harness.move_pointer(cappy_core::Point::new(1.0, 1.0));
    let partial_frame = harness.frame();
    assert!(matches!(partial_frame.damage, Damage::Regions(_)));
    let partial = incremental.render(&partial_frame, background()).unwrap();

    let full = renderer()
        .render(&full_repaint(&partial_frame), background())
        .unwrap();
    assert_eq!(partial.count_differences(&full, 0), 0);
    assert_ne!(probe(&partial, &harness, "label"), [0; 4]);
}

#[test]
fn idle_frames_keep_the_previous_pixels() {
    let mut harness = harness(counter(signal(0)));
    let mut renderer = renderer();
    let first = renderer.render(&harness.frame(), background()).unwrap();
    let idle_frame = harness.frame();
    assert_eq!(idle_frame.damage, Damage::None);
    let second = renderer.render(&idle_frame, background()).unwrap();
    assert_eq!(first, second);
}
