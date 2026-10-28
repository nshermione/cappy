use cappy_core::{Damage, Element, Signal, Theme, UpdateOutcome, signal};
use cappy_dsl::ui;
use cappy_testing::TestUi;

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

#[test]
fn clicking_updates_the_label() {
    let count = signal(0);
    let mut harness = TestUi::new(counter(count), Theme::light());
    harness.click("increment");
    harness.click("increment");
    harness.update();
    assert_eq!(count.get_untracked(), 2);
    assert_eq!(harness.text("label"), "Count: 2");
}

#[test]
fn click_damage_stays_within_the_row() {
    let mut harness = TestUi::new(counter(signal(0)), Theme::light());
    harness.click("increment");
    let Damage::Regions(regions) = harness.damage() else {
        panic!("a click should cause partial damage");
    };
    let row = harness.rect("row");
    assert!(regions.iter().all(|region| row.contains_rect(region)));
}

#[test]
fn ui_is_idle_once_settled() {
    let mut harness = TestUi::new(counter(signal(0)), Theme::light());
    assert_eq!(harness.update(), UpdateOutcome::Idle);
    assert_eq!(harness.damage(), Damage::None);
}

#[test]
fn text_falls_back_to_the_first_text_child() {
    let harness = TestUi::new(counter(signal(0)), Theme::light());
    assert_eq!(harness.text("increment"), "+1");
}
