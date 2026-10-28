//! Pixel tests with the CPU renderer; no GPU needed. Run with
//! `cargo test -p cappy-testing --features cpu-tests`.
#![cfg(feature = "cpu-tests")]

mod common;

use std::path::Path;

use cappy_core::{Damage, Point, Theme, signal};
use cappy_testing::assert_snapshot;
use cappy_testing::cpu::CpuHeadlessRenderer;
use common::{background, counter, fonts, full_repaint, harness, shapes};

/// The CPU renderer is deterministic, so snapshots must match exactly.
const SNAPSHOT_TOLERANCE: u8 = 0;

fn renderer() -> CpuHeadlessRenderer {
    CpuHeadlessRenderer::new(fonts())
}

#[test]
fn shapes_match_snapshot() {
    let mut harness = harness(shapes());
    let image = renderer().render(&harness.frame(), background());
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/cpu");
    assert_snapshot(&snapshots, "shapes", &image, SNAPSHOT_TOLERANCE);
}

#[test]
fn colors_land_where_layout_put_them() {
    let mut harness = harness(counter(signal(0)));
    let image = renderer().render(&harness.frame(), background());
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
    let image = renderer().render(&harness.frame(), background());
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
    let mut harness = harness(counter(signal(0)));
    let mut incremental = renderer();
    incremental.render(&harness.frame(), background());

    harness.click("increment");
    harness.move_pointer(Point::new(1.0, 1.0));
    let partial_frame = harness.frame();
    assert!(matches!(partial_frame.damage, Damage::Regions(_)));
    let partial = incremental.render(&partial_frame, background());
    assert!(
        !incremental.renderer().last_repainted().is_empty(),
        "the click repaints something"
    );

    let full = renderer().render(&full_repaint(&partial_frame), background());
    assert_eq!(partial.count_differences(&full, 0), 0);
}

#[test]
fn idle_frames_keep_the_previous_pixels() {
    let mut harness = harness(counter(signal(0)));
    let mut renderer = renderer();
    let first = renderer.render(&harness.frame(), background());
    let idle_frame = harness.frame();
    assert_eq!(idle_frame.damage, Damage::None);
    let second = renderer.render(&idle_frame, background());
    assert_eq!(first, second);
}
