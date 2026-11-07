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
    assert_eq!(harness.text("task-title-4"), "Added 4");

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

#[test]
fn settled_scene_is_idle() {
    let mut harness = harness();
    assert_eq!(harness.damage(), Damage::None);
}
