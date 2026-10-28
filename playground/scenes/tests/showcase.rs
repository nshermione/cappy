use cappy::Damage;
use cappy_testing::TestUi;
use playground_scenes::showcase;

fn harness() -> TestUi {
    TestUi::from_ui(showcase())
}

#[test]
fn counter_buttons_change_the_count() {
    let mut harness = harness();
    harness.click("increment");
    harness.click("increment");
    harness.click("decrement");
    harness.update();
    assert_eq!(harness.text("count"), "Count: 1");
    harness.click("reset");
    harness.update();
    assert_eq!(harness.text("count"), "Count: 0");
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
    harness.click("tile-3");
    harness.update();
    assert_eq!(harness.text("selection"), "Selected tile 4");
}

#[test]
fn tasks_toggle_between_todo_and_done() {
    let mut harness = harness();
    harness.click("task-1");
    harness.update();
    let row = harness.node("task-1");
    let badge = harness.ui().node_children(row)[0];
    let label = harness.ui().node_children(badge)[0];
    assert_eq!(harness.ui().node_text(label), Some("done"));
}

#[test]
fn settled_scene_is_idle() {
    let mut harness = harness();
    assert_eq!(harness.damage(), Damage::None);
}
