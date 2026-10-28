//! The GPU and CPU renderers must draw the same scene almost identically. They blend in
//! different color spaces (linear vs sRGB), so anti-aliased edges and glyphs may differ a little;
//! everything else must match. Run with
//! `cargo test -p cappy-testing --features gpu-tests,cpu-tests`.
#![cfg(all(feature = "gpu-tests", feature = "cpu-tests"))]

mod common;

use cappy_core::{Element, signal};
use cappy_testing::Image;
use cappy_testing::cpu::CpuHeadlessRenderer;
use cappy_testing::gpu::HeadlessRenderer;
use common::{VIEWPORT, background, counter, fonts, harness, shapes};

/// Channel difference treated as equal; covers rounding between the two pipelines.
const CHANNEL_TOLERANCE: u8 = 3;
/// Share of pixels allowed to differ more, for anti-aliased edges and glyph coverage.
const MAX_DIFFERING_SHARE: f64 = 0.02;

fn render_both(root: Element) -> (Image, Image) {
    let mut harness = harness(root);
    let frame = harness.frame();
    let gpu = HeadlessRenderer::new(fonts())
        .expect("headless GPU available")
        .render(&frame, background())
        .expect("GPU render succeeds");
    let cpu = CpuHeadlessRenderer::new(fonts()).render(&frame, background());
    (gpu, cpu)
}

fn assert_close(name: &str, gpu: &Image, cpu: &Image) {
    let total = (VIEWPORT.physical_width * VIEWPORT.physical_height) as f64;
    let differing = gpu.count_differences(cpu, CHANNEL_TOLERANCE);
    let share = differing as f64 / total;
    assert!(
        share <= MAX_DIFFERING_SHARE,
        "{name}: {differing} pixels ({:.2}%) differ between the GPU and CPU renderers",
        share * 100.0
    );
}

#[test]
fn shapes_look_the_same_on_both_renderers() {
    let (gpu, cpu) = render_both(shapes());
    assert_close("shapes", &gpu, &cpu);
}

#[test]
fn text_and_buttons_look_the_same_on_both_renderers() {
    let (gpu, cpu) = render_both(counter(signal(0)));
    assert_close("counter", &gpu, &cpu);
}
