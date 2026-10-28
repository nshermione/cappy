//! Pixel tests on a real or software GPU. Run with `cargo test -p cappy-testing --features gpu-tests`.
#![cfg(feature = "gpu-tests")]

mod common;

use std::path::Path;

use cappy_core::{Damage, Theme, signal};
use cappy_testing::TestUi;
use cappy_testing::gpu::{HeadlessRenderer, Image, assert_snapshot};
use common::{background, counter, fonts, full_repaint, harness, shapes};

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
