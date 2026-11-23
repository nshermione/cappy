//! Pixel tests on a real or software GPU. Run with `cargo test -p cappy-testing --features gpu-tests`.
#![cfg(feature = "gpu-tests")]

mod common;

use std::path::Path;

use cappy_core::{Damage, Theme};
use cappy_testing::TestUi;
use cappy_testing::gpu::{HeadlessRenderer, Image, assert_snapshot};
use common::{background, context, counter, effects, fonts, full_repaint, harness, shapes};

/// Channel difference allowed between GPUs for anti-aliased edges.
const SNAPSHOT_TOLERANCE: u8 = 2;

fn renderer() -> HeadlessRenderer {
    HeadlessRenderer::new(fonts()).expect("headless GPU available")
}

fn probe(image: &Image, harness: &TestUi, key: &str) -> [u8; 4] {
    let center = harness.center(key);
    image.pixel(center.x as u32, center.y as u32)
}

#[test]
fn shapes_match_snapshot() {
    let cx = context();
    let mut harness = harness(&cx, shapes(&cx));
    let image = renderer().render(&harness.frame(), background()).unwrap();
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    assert_snapshot(&snapshots, "shapes", &image, SNAPSHOT_TOLERANCE);
}

#[test]
fn effects_match_snapshot() {
    let cx = context();
    let mut harness = harness(&cx, effects(&cx));
    let image = renderer().render(&harness.frame(), background()).unwrap();
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    assert_snapshot(&snapshots, "effects", &image, SNAPSHOT_TOLERANCE);
}

#[test]
fn colors_land_where_layout_put_them() {
    let cx = context();
    let mut harness = harness(&cx, counter(&cx, cx.reactive(0)));
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
    let cx = context();
    let mut harness = harness(&cx, counter(&cx, cx.reactive(0)));
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
    let cx = context();
    let count = cx.reactive(0i32);
    let mut harness = harness(&cx, counter(&cx, count));
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
    let cx = context();
    let mut harness = harness(&cx, counter(&cx, cx.reactive(0)));
    let mut renderer = renderer();
    let first = renderer.render(&harness.frame(), background()).unwrap();
    let idle_frame = harness.frame();
    assert_eq!(idle_frame.damage, Damage::None);
    let second = renderer.render(&idle_frame, background()).unwrap();
    assert_eq!(first, second);
}

#[test]
fn a_scaled_svg_circle_stays_crisp() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cappy-core/fixtures/sample.svg"
    );
    let cx = context();
    let mut harness = harness(
        &cx,
        cappy_core::image(path).key("svg").style(
            cappy_core::StyleRefinement::default()
                .width(256)
                .height(192),
        ),
    );
    let frame = harness.frame();
    let vector = frame
        .primitives
        .iter()
        .find_map(|primitive| match primitive {
            cappy_core::Primitive::Vector(vector) => Some(vector),
            _ => None,
        })
        .expect("svg paints as vectors");
    assert!(
        vector.rect.width >= 200.0,
        "layout width {}",
        vector.rect.width
    );
    let placement = vector.placement();
    let scale = frame.scale_factor;
    let map = |x: f32, y: f32| -> (u32, u32) {
        let u = x / vector.picture.view_width;
        let v = y / vector.picture.view_height;
        let span_u = (placement.uv_max_x - placement.uv_min_x).max(1.0e-4);
        let span_v = (placement.uv_max_y - placement.uv_min_y).max(1.0e-4);
        let px =
            (placement.dest.x + (u - placement.uv_min_x) / span_u * placement.dest.width) * scale;
        let py =
            (placement.dest.y + (v - placement.uv_min_y) / span_v * placement.dest.height) * scale;
        (px.round() as u32, py.round() as u32)
    };
    let image = renderer().render(&frame, background()).unwrap();
    let (inside_x, inside_y) = map(33.0, 24.0);
    let (outside_x, outside_y) = map(35.0, 24.0);
    let inside = image.pixel(inside_x, inside_y);
    let outside = image.pixel(outside_x, outside_y);
    assert!(
        inside[0] > 220 && inside[2] < 80,
        "inside {inside:?} at {inside_x},{inside_y}"
    );
    assert!(
        outside[0] < 90 && outside[2] > 200,
        "outside {outside:?} at {outside_x},{outside_y}"
    );
}
