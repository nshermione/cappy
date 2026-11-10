#[cfg(not(feature = "cpu"))]
use cappy::Primitive;
use cappy::{Application, Background, Damage};
use cappy_testing::TestUi;
use playground_scenes::showcase;

fn harness() -> TestUi {
    let application = Application::new();
    TestUi::from_ui(showcase(application.context()))
}

fn open_tab(harness: &mut TestUi, tab: &str) {
    harness.click(tab);
    harness.update();
}

#[test]
fn counter_buttons_change_the_count() {
    let mut harness = harness();
    harness.click("increment");
    harness.click("increment");
    harness.click("decrement");
    harness.update();
    assert_eq!(harness.text("count"), "Count: 1");
    assert_eq!(harness.text("count-mirror"), "Count: 1");
    assert_eq!(harness.text("count-local"), "Local: 0");
    harness.click("reset");
    harness.update();
    assert_eq!(harness.text("count"), "Count: 0");
    assert_eq!(harness.text("count-mirror"), "Count: 0");
    harness.click("local-increment");
    harness.update();
    assert_eq!(harness.text("count"), "Count: 0");
    assert_eq!(harness.text("count-mirror"), "Count: 0");
    assert_eq!(harness.text("count-local"), "Local: 1");
}

#[test]
fn counter_click_repaints_only_part_of_the_window() {
    let mut harness = harness();
    harness.click("increment");
    assert!(matches!(harness.damage(), Damage::Regions(_)));
}

#[test]
fn theme_button_cycles_themes_without_moving_nodes() {
    let mut harness = harness();
    let count_rect = harness.rect("count");
    let names: Vec<String> = (0..3)
        .map(|_| {
            harness.click("theme-toggle");
            harness.update();
            harness.ui().theme().name().to_owned()
        })
        .collect();
    assert_eq!(names, ["dark", "ocean", "light"]);
    assert_eq!(harness.rect("count"), count_rect);
}

#[test]
fn selecting_a_tile_updates_the_caption() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-tiles");
    harness.click("tile-3");
    harness.update();
    assert_eq!(harness.text("selection"), "Selected tile 4");
}

#[test]
fn tasks_toggle_between_todo_and_done() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-checklist");
    harness.click("task-title-1");
    harness.update();
    let row = harness.node("task-1");
    let badge = harness.ui().node_children(row)[0];
    let label = harness.ui().node_children(badge)[0];
    assert_eq!(harness.ui().node_text(label), Some("done"));
}

#[test]
fn show_keeps_its_count_and_if_starts_over() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-checklist");
    harness.click("show-increment");
    harness.update();
    assert_eq!(harness.text("show-count"), "Kept: 1");
    let kept = harness.node("show-count");

    harness.click("show-toggle");
    harness.update();
    assert_eq!(harness.node("show-count"), kept);
    assert_eq!(harness.rect("show-count").width, 0.0);
    assert_eq!(harness.text("show-count"), "Kept: 1");

    harness.click("show-toggle");
    harness.update();
    assert_eq!(harness.node("show-count"), kept);
    assert_eq!(harness.text("show-count"), "Kept: 1");
    assert!(harness.rect("show-count").width > 0.0);

    harness.click("branch-increment");
    harness.update();
    assert_eq!(harness.text("branch-count"), "Draft: 1");
    harness.click("branch-toggle");
    harness.update();
    assert_eq!(harness.text("branch-count"), "Sent: 0");
    harness.click("branch-toggle");
    harness.update();
    assert_eq!(harness.text("branch-count"), "Draft: 0");
}

#[test]
fn checklist_rows_extend_past_the_scrollport() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-checklist");
    let list = harness.rect("tasks");
    let first = harness.rect("task-0");
    let last = harness.rect("task-19");
    assert!(
        (first.y - list.y).abs() < 1.0,
        "first row should start at the scrollport, first {first:?} list {list:?}"
    );
    assert!(
        list.bottom() <= 600.0,
        "the scrollport should stay inside the window, got {list:?}"
    );
    assert!(
        last.bottom() > list.bottom(),
        "the checklist column should be taller than its scrollport"
    );
}

#[test]
fn for_keeps_a_task_row_when_it_moves() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-checklist");
    harness.click("task-title-1");
    harness.update();
    harness.click("task-up-1");
    harness.update();

    let tasks = harness.node("tasks");
    let children = harness.ui().node_children(tasks);
    assert_eq!(children[0], harness.node("task-1"));
    let badge = harness.ui().node_children(children[0])[0];
    let label = harness.ui().node_children(badge)[0];
    assert_eq!(harness.ui().node_text(label), Some("done"));

    harness.click("add-task");
    harness.update();
    assert_eq!(harness.text("task-title-20"), "Added 20");

    harness.click("task-drop-0");
    harness.update();
    assert!(harness.ui().find_by_key("task-0").is_none());
    assert_eq!(
        harness.text("task-title-1"),
        "Stay idle when nothing changes"
    );
}

#[test]
fn badge_sits_with_the_counter() {
    let mut harness = harness();

    let badge = harness.node("counter-badge");
    let children = harness.ui().node_children(badge).to_vec();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], harness.node("badge-mark"));
    assert_eq!(harness.ui().node_text(children[0]), Some("•"));
    assert_eq!(harness.ui().node_text(children[1]), Some("Badge (0) c:0"));
    assert_eq!(children[2], harness.node("badge-tail"));
    assert_eq!(harness.ui().node_text(children[2]), Some("end"));

    harness.click("counter-badge");
    harness.update();
    assert_eq!(harness.ui().node_text(children[1]), Some("Badge (1) c:1"));
    assert_eq!(harness.text("count"), "Count: 1");
    assert_eq!(harness.text("count-mirror"), "Count: 1");
    assert_eq!(harness.text("count-local"), "Local: 0");
}

#[test]
fn paint_tab_uses_gradient_shadow_and_image() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-paint");

    let gradient = harness
        .ui()
        .node_style(harness.node("gradient-right"))
        .unwrap();
    assert!(matches!(
        gradient.visual.background,
        Background::Linear { .. }
    ));
    assert!(
        harness
            .ui()
            .node_style(harness.node("shadow-soft"))
            .unwrap()
            .visual
            .shadow
            .is_visible()
    );
    assert_eq!(harness.text("shadow-state"), "Resting");
    let resting = harness
        .ui()
        .node_style(harness.node("shadow-card"))
        .unwrap()
        .visual
        .shadow
        .offset_y;

    harness.click("shadow-toggle");
    assert!(matches!(harness.damage(), Damage::Regions(_)));
    harness.update();
    assert_eq!(harness.text("shadow-state"), "Lifted");
    let lifted = harness
        .ui()
        .node_style(harness.node("shadow-card"))
        .unwrap()
        .visual
        .shadow
        .offset_y;
    assert!(lifted > resting);

    assert!(harness.rect("picture-contain").width > 0.0);
    assert!(harness.rect("picture-cover").width > 0.0);
    assert!(harness.rect("picture-fill").width > 0.0);
}

#[cfg(not(feature = "cpu"))]
#[test]
fn motion_specimens_are_spaced_and_the_list_scrolls() {
    let mut harness = harness();
    open_tab(&mut harness, "tab-motion");
    let color = harness.rect("motion-color");
    let scale = harness.rect("motion-scale");
    let spin = harness.rect("motion-spin");
    assert!(
        scale.x >= color.right() + 48.0,
        "scale should sit clear of color, color {color:?} scale {scale:?}"
    );
    assert!(
        spin.x >= scale.right() + 48.0,
        "spin should sit clear of scale, scale {scale:?} spin {spin:?}"
    );
    let list = harness.rect("motion-scroll");
    let last = harness.rect("ease-css-in-out");
    assert!(
        list.bottom() <= 600.0,
        "the motion scrollport should stay inside the window, got {list:?}"
    );
    assert!(
        last.bottom() > list.bottom(),
        "the ease column should extend past the scrollport, last {last:?} list {list:?}"
    );
}

#[cfg(not(feature = "cpu"))]
#[test]
fn motion_tab_fades_and_slides_from_manual_animate() {
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    let mut harness = harness();
    open_tab(&mut harness, "tab-motion");
    let slide_y = harness.rect("motion-slide").y;

    harness.click("motion-fade");
    harness.click("motion-shift");
    harness.update();
    assert!(harness.ui().frame_deadline(Instant::now()).is_some());
    assert!((harness.rect("motion-slide").y - slide_y).abs() < 0.5);

    sleep(Duration::from_millis(120));
    harness.update();
    assert!(harness.rect("motion-slide").y > slide_y + 8.0);
    assert!(fill_alpha(&harness, "motion-opacity") < 0.95);
}

#[cfg(not(feature = "cpu"))]
#[test]
fn sequence_and_parallel_replay_on_their_own_boxes() {
    use std::thread::sleep;
    use std::time::Duration;

    let mut harness = harness();
    open_tab(&mut harness, "tab-motion");
    reveal(&mut harness, "motion-sequence");
    let sequence_y = harness.rect("motion-sequence-box").y;
    let parallel_y = harness.rect("motion-parallel-box").y;
    let slide_y = harness.rect("motion-slide").y;
    let fade_alpha = fill_alpha(&harness, "motion-opacity");

    harness.click("motion-sequence");
    harness.update();
    sleep(Duration::from_millis(160));
    harness.update();
    assert!((harness.rect("motion-sequence-box").y - sequence_y).abs() < 0.5);
    assert!(fill_alpha(&harness, "motion-sequence-box") < 0.9);
    assert!((harness.rect("motion-slide").y - slide_y).abs() < 0.5);
    assert!((fill_alpha(&harness, "motion-opacity") - fade_alpha).abs() < 0.02);
    assert!((harness.rect("motion-parallel-box").y - parallel_y).abs() < 0.5);

    harness.click("motion-sequence");
    harness.update();
    assert!(fill_alpha(&harness, "motion-sequence-box") > 0.95);
    assert!((harness.rect("motion-sequence-box").y - sequence_y).abs() < 0.5);

    sleep(Duration::from_millis(160));
    harness.update();
    assert!((harness.rect("motion-sequence-box").y - sequence_y).abs() < 0.5);
    assert!(fill_alpha(&harness, "motion-sequence-box") < 0.9);

    harness.click("motion-parallel");
    harness.update();
    sleep(Duration::from_millis(160));
    harness.update();
    assert!(harness.rect("motion-parallel-box").y > parallel_y + 8.0);
    assert!(fill_alpha(&harness, "motion-parallel-box") < 0.9);
    assert!((harness.rect("motion-slide").y - slide_y).abs() < 0.5);
    assert!((fill_alpha(&harness, "motion-opacity") - fade_alpha).abs() < 0.02);

    harness.click("motion-parallel");
    harness.update();
    assert!((harness.rect("motion-parallel-box").y - parallel_y).abs() < 0.5);
    assert!(fill_alpha(&harness, "motion-parallel-box") > 0.95);
}

/// Wheels the motion list until `key` sits inside the scrollport. The ease only moves on frames
/// close together, so this steps the clock instead of waiting out one long sleep.
#[cfg(not(feature = "cpu"))]
fn reveal(harness: &mut TestUi, key: &str) {
    use std::thread::sleep;
    use std::time::Duration;

    use cappy::{InputEvent, Point};

    for _ in 0..30 {
        let list = harness.rect("motion-scroll");
        let rect = harness.rect(key);
        if rect.y >= list.y + 1.0 && rect.bottom() <= list.bottom() - 1.0 {
            return;
        }
        let delta = if rect.bottom() > list.bottom() {
            -120.0
        } else {
            120.0
        };
        harness.move_pointer(Point::new(list.x + 24.0, list.y + 24.0));
        harness.ui_mut().handle_event(InputEvent::Wheel {
            delta: Point::new(0.0, delta),
        });
        for _ in 0..12 {
            sleep(Duration::from_millis(16));
            harness.update();
        }
    }
    panic!("could not scroll {key} into the motion list");
}

#[cfg(not(feature = "cpu"))]
fn fill_alpha(harness: &TestUi, key: &str) -> f32 {
    let rect = harness.rect(key);
    harness
        .ui()
        .display_list()
        .iter()
        .find_map(|primitive| {
            let Primitive::Quad(quad) = primitive else {
                return None;
            };
            if quad.rect != rect {
                return None;
            }
            let Background::Solid(color) = quad.background else {
                return None;
            };
            Some(color.a)
        })
        .expect("solid fill")
}

#[cfg(not(feature = "cpu"))]
#[test]
fn motion_loops_are_already_running() {
    use std::thread::sleep;
    use std::time::Duration;

    let mut harness = harness();
    open_tab(&mut harness, "tab-motion");
    let fly_y = harness.rect("motion-fly").y;
    let dot_x = harness.rect("ease-linear").x;
    assert!(
        harness
            .ui()
            .frame_deadline(std::time::Instant::now())
            .is_some()
    );

    sleep(Duration::from_millis(200));
    harness.update();
    assert!(harness.rect("motion-fly").y < fly_y - 6.0);
    assert!(harness.rect("ease-linear").x > dot_x + 12.0);
    let spin = harness
        .ui()
        .display_list()
        .iter()
        .find_map(|primitive| {
            let Primitive::Quad(quad) = primitive else {
                return None;
            };
            let card = harness.rect("motion-spin");
            let same = (quad.rect.x - card.x).abs() < 0.5 && (quad.rect.y - card.y).abs() < 0.5;
            same.then_some(quad.spin.radians)
        })
        .expect("spin quad");
    assert!(spin > 0.2);
}

/// The `cpu` feature removes motion from cappy, so this scene has no Motion tab.
#[cfg(feature = "cpu")]
#[test]
fn cpu_feature_omits_the_motion_tab() {
    let harness = harness();
    assert!(harness.ui().find_by_key("tab-motion").is_none());
    assert!(harness.ui().find_by_key("tab-counter").is_some());
}

#[test]
fn settled_scene_is_idle() {
    let mut harness = harness();
    assert_eq!(harness.damage(), Damage::None);
}
