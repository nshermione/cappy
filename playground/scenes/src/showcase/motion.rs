//! The motion tab. Loops start on their own. Fade and Slide still call [`Context::animate`].

use std::time::Duration;

use cappy::Class as ThemeClass;
use cappy::MotionCtx;
use cappy::prelude::*;

use super::style::Class;

const TRAVEL: f32 = 150.0;
/// Length of one leg. Left to right is one cycle, right to left is the next, both this long.
const EASE_LEG: Duration = Duration::from_millis(1100);

struct EaseRow {
    key: &'static str,
    label: &'static str,
    ease: Ease,
}

fn ease_rows() -> Vec<EaseRow> {
    vec![
        row("ease-linear", "Linear", Ease::Linear),
        row("ease-in-quad", "InQuad", Ease::InQuad),
        row("ease-out-quad", "OutQuad", Ease::OutQuad),
        row("ease-in-out-quad", "InOutQuad", Ease::InOutQuad),
        row("ease-in-cubic", "InCubic", Ease::InCubic),
        row("ease-out-cubic", "OutCubic", Ease::OutCubic),
        row("ease-in-out-cubic", "InOutCubic", Ease::InOutCubic),
        row("ease-in-sine", "InSine", Ease::InSine),
        row("ease-out-sine", "OutSine", Ease::OutSine),
        row("ease-in-out-sine", "InOutSine", Ease::InOutSine),
        row("ease-in-expo", "InExpo", Ease::InExpo),
        row("ease-out-expo", "OutExpo", Ease::OutExpo),
        row("ease-in-out-expo", "InOutExpo", Ease::InOutExpo),
        row("ease-in-back", "InBack", Ease::InBack),
        row("ease-out-back", "OutBack", Ease::OutBack),
        row("ease-in-out-back", "InOutBack", Ease::InOutBack),
        row("ease-out-bounce", "OutBounce", Ease::OutBounce),
        row("ease-css", "css ease", Ease::css_ease()),
        row("ease-css-in", "css ease-in", Ease::css_ease_in()),
        row("ease-css-out", "css ease-out", Ease::css_ease_out()),
        row(
            "ease-css-in-out",
            "css ease-in-out",
            Ease::css_ease_in_out(),
        ),
    ]
}

fn row(key: &'static str, label: &'static str, ease: Ease) -> EaseRow {
    EaseRow { key, label, ease }
}

#[component]
pub struct MotionTab {
    faded: bool,
    shifted: bool,
}

impl Component for MotionTab {
    fn render(&self) -> Element {
        let cx = Context::current();
        let tracks = ease_section();
        let playback = playback_row(cx.clone());
        element! {
            Container(class = [ThemeClass::Card, Class::Grow]) {
                Text("Motion", class = ThemeClass::Subtitle)
                Text(
                    "Color, scale, spin, shake and fly loop on their own. Fade and Slide are one shot.",
                    class = ThemeClass::Muted,
                )
                Container(class = Class::ScrollList, key = "motion-scroll") {
                    Container(class = Class::MotionRow) {
                        { opacity_card(cx.clone(), self.faded) }
                        { slide_card(cx, self.shifted) }
                    }
                    Container(class = Class::MotionRow) {
                        { swatch(Class::MotionColor, "motion-color", "color", false) }
                        { swatch(Class::MotionScale, "motion-scale", "scale", false) }
                        { swatch(Class::MotionSpin, "motion-spin", "spin", true) }
                        { swatch(Class::MotionTravel, "motion-shake", "shake", false) }
                        { swatch(Class::MotionTravel, "motion-fly", "fly", false) }
                    }
                    { playback }
                    Text("Easing", class = ThemeClass::Subtitle)
                    Text(
                        "Left to right is one cycle, right to left is the next. Both use the same duration.",
                        class = ThemeClass::Muted,
                    )
                    Container(class = Class::MotionEases) {
                        { tracks }
                    }
                }
            }
        }
    }
}

pub(super) fn spawn_loops(ui: &mut MotionCtx) {
    let ms = Duration::from_millis;
    play(
        ui,
        "motion-color",
        Motion::background(Color::hex("#f97316"))
            .duration(ms(900))
            .ease(Ease::InOutSine)
            .ping_pong(),
    );
    play(
        ui,
        "motion-scale",
        Motion::scale(1.28)
            .duration(ms(800))
            .ease(Ease::InOutSine)
            .ping_pong(),
    );
    play(
        ui,
        "motion-spin",
        Motion::rotation(360.0)
            .duration(ms(1600))
            .ease(Ease::Linear)
            .looped(),
    );
    play(
        ui,
        "motion-shake",
        Motion::shake(6.0).duration(ms(180)).looped(),
    );
    play(
        ui,
        "motion-fly",
        Motion::fly(0.0, -22.0)
            .duration(ms(700))
            .ease(Ease::OutCubic)
            .ping_pong(),
    );
    for ease in ease_rows() {
        play(
            ui,
            ease.key,
            Motion::translation(TRAVEL, 0.0)
                .duration(EASE_LEG)
                .ease(ease.ease)
                .ping_pong(),
        );
    }
}

fn play(ui: &mut MotionCtx, key: &str, motion: Motion) {
    let Some(id) = ui.find_by_key(key) else {
        return;
    };
    ui.animate(id, motion);
}

fn swatch(class: Class, key: &'static str, label: &'static str, pip: bool) -> Element {
    let mark = if pip {
        element! { Container(class = Class::MotionPip) }
    } else {
        element! { Text(label) }
    };
    element! {
        Container(class = Class::Specimen) {
            Container(class = class, key = key) {
                { mark }
            }
            Text(label, class = ThemeClass::Muted)
        }
    }
}

fn opacity_card(cx: Context, faded: Reactive<bool>) -> Element {
    element! {
        Container(class = Class::Specimen) {
            Container(class = Class::MotionOpacity, key = "motion-opacity") {
                Text("opacity")
            }
            Container(
                class = ThemeClass::Btn,
                key = "motion-fade",
                on_click = move |_| {
                    let next = !faded.get();
                    faded.set(next);
                    let opacity = if next { 0.2 } else { 1.0 };
                    cx.animate(move |ui| {
                        let id = ui.find_by_key("motion-opacity").expect("opacity card");
                        ui.animate(
                            id,
                            Motion::opacity(opacity)
                                .duration(Duration::from_millis(280))
                                .ease(Ease::OutCubic),
                        );
                    });
                },
            ) {
                Text("Fade")
            }
        }
    }
}

fn slide_card(cx: Context, shifted: Reactive<bool>) -> Element {
    element! {
        Container(class = Class::Specimen) {
            Container(class = Class::MotionSlide, key = "motion-slide") {
                Text("transform")
            }
            Container(
                class = ThemeClass::Btn,
                key = "motion-shift",
                on_click = move |_| {
                    let next = !shifted.get();
                    shifted.set(next);
                    let y = if next { 28.0 } else { 0.0 };
                    cx.animate(move |ui| {
                        let id = ui.find_by_key("motion-slide").expect("slide card");
                        ui.animate(
                            id,
                            Motion::translation(0.0, y)
                                .duration(Duration::from_millis(360))
                                .ease(Ease::OutBack),
                        );
                    });
                },
            ) {
                Text("Slide")
            }
        }
    }
}

fn playback_row(cx: Context) -> Element {
    let parallel = cx.clone();
    let ms = Duration::from_millis;
    element! {
        Container(class = Class::MotionRow) {
            Container(
                class = ThemeClass::Btn,
                key = "motion-sequence",
                on_click = move |_| {
                    let cx = cx.clone();
                    cx.animate(move |ui| {
                        let Some(opacity) = ui.find_by_key("motion-opacity") else { return };
                        let Some(slide) = ui.find_by_key("motion-slide") else { return };
                        ui.play(Anim::sequence([
                            Anim::play(
                                opacity,
                                Motion::opacity(0.25).duration(ms(280)).ease(Ease::OutCubic),
                            ),
                            Anim::play(
                                slide,
                                Motion::translation(0.0, 28.0)
                                    .duration(ms(360))
                                    .ease(Ease::OutBack)
                                    .delay(ms(120)),
                            ),
                        ]));
                    });
                },
            ) {
                Text("Sequence")
            }
            Container(
                class = ThemeClass::Btn,
                key = "motion-parallel",
                on_click = move |_| {
                    let parallel = parallel.clone();
                    parallel.animate(move |ui| {
                        let Some(opacity) = ui.find_by_key("motion-opacity") else { return };
                        let Some(slide) = ui.find_by_key("motion-slide") else { return };
                        ui.play(Anim::together([
                            Anim::play(
                                opacity,
                                Motion::opacity(0.25).duration(ms(280)).ease(Ease::OutCubic),
                            ),
                            Anim::play(
                                slide,
                                Motion::translation(0.0, 28.0)
                                    .duration(ms(360))
                                    .ease(Ease::OutBack),
                            ),
                        ]));
                    });
                },
            ) {
                Text("Parallel")
            }
        }
    }
}

fn ease_section() -> Element {
    fragment(ease_rows().into_iter().map(ease_track))
}

fn ease_track(row: EaseRow) -> Element {
    let key = row.key;
    element! {
        Container(class = ThemeClass::Row) {
            Container(class = Class::EaseName) {
                Text(row.label, class = ThemeClass::Muted)
            }
            Container(class = Class::EaseTrack) {
                Container(class = Class::EaseDot, key = key)
            }
        }
    }
}
