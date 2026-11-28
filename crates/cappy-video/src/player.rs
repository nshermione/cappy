//! The default player. Hover the picture for play, seek, sound, quality, speed, and fill.
//! Opening a locator happens off the UI thread. Sound is on unless [`Player::audio`] turns it off.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cappy_core::{
    Align, Class, Color, Component, Context, CursorIcon, Dynamic, Ease, Element, FlexDirection,
    IntoElement, Justify, Listener, Motion, ObjectFit, Overflow, Point, PointerButton,
    PointerEvents, PointerPhase, PropInput, Reactive, Rect, RetirePlaceholder, StyleRefinement,
    VideoQuality, VideoSource, div, fragment, if_else, image, on_mounted, percent, reactive, show,
    text, text_dyn, video,
};
use cappy_dsl::element;

use crate::file::OpenOptions;
use crate::scrub::scrub_pts;

const BAR_HEIGHT: f32 = 48.0;
const SPINNER: f32 = 48.0;
const SPINNER_DOT: f32 = 6.0;
const SPINNER_DOTS: usize = 28;
const SPINNER_SWEEP: f32 = 300.0;
const PLAY: &str = "▶";
const PAUSE: &str = "❚❚";
const SETTINGS: &str = "⚙";
const FULL: &str = "⛶";
const FIT: &str = "❐";
const RED: Color = Color::rgba(255, 0, 0, 255);
const MENU_ROW: f32 = 28.0;
const MENU_WIDTH: f32 = 200.0;
const MENU_PAD: f32 = 8.0;
const SPEEDS: [f32; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// A player for a path, an `.m3u8`, or an `.mpd`.
///
/// Controls and sound are on. [`controls`](Player::controls) hides the bar.
/// [`audio`](Player::audio) skips the device and the audio decoder.
/// [`on_ready`](Player::on_ready) hands over the [`VideoSource`] for a custom bar.
/// [`open`](crate::open) is the picture only: no device, no keyframe scan.
/// [`open_with`](crate::open_with) turns those on without this shell.
///
/// Call this while building a tree, then [`IntoElement::into_element`].
#[must_use]
pub fn player(locator: impl Into<String>) -> Player {
    Player::open().locator(locator.into())
}

/// Built-in picture and, unless [`controls`](Self::controls) is off, the hover bar.
#[derive(Clone)]
pub struct Player {
    locator: Reactive<String>,
    controls: Reactive<bool>,
    audio: Reactive<bool>,
    ready: Listener<VideoSource>,
}

impl Player {
    fn open() -> Self {
        Self {
            locator: Reactive::placeholder(String::new()),
            controls: Reactive::placeholder(true),
            audio: Reactive::placeholder(true),
            ready: Listener::empty(),
        }
    }

    fn locator<Value, Marker>(mut self, value: Value) -> Self
    where
        Value: PropInput<String, Marker>,
    {
        let previous = self.locator;
        PropInput::apply(value, &mut self.locator);
        if previous != self.locator {
            RetirePlaceholder::dispose_if_placeholder(previous);
        }
        self
    }

    /// `false` leaves the picture and drops the built-in bar. Sound still follows [`audio`](Self::audio).
    #[must_use]
    pub fn controls<Value, Marker>(mut self, value: Value) -> Self
    where
        Value: PropInput<bool, Marker>,
    {
        let previous = self.controls;
        PropInput::apply(value, &mut self.controls);
        if previous != self.controls {
            RetirePlaceholder::dispose_if_placeholder(previous);
        }
        self
    }

    /// `false` does not open a device and does not decode audio packets.
    #[must_use]
    pub fn audio<Value, Marker>(mut self, value: Value) -> Self
    where
        Value: PropInput<bool, Marker>,
    {
        let previous = self.audio;
        PropInput::apply(value, &mut self.audio);
        if previous != self.audio {
            RetirePlaceholder::dispose_if_placeholder(previous);
        }
        self
    }

    /// Runs on the UI thread once the locator has opened.
    #[must_use]
    pub fn on_ready(self, handler: impl FnMut(&VideoSource) + 'static) -> Self {
        self.ready.set(handler);
        self
    }
}

impl Component for Player {
    fn render(&self) -> Element {
        let opened: Reactive<Option<Result<VideoSource, String>>> = reactive(None);
        let fill = reactive(false);
        let locator = self.locator.get();
        let hear = self.audio.get();
        let watch = opened.clone();
        let ready = self.ready.clone();
        on_mounted(move || {
            arm_spinner();
            let slot = Arc::new(Mutex::new(None));
            let writer = Arc::clone(&slot);
            let path = locator.clone();
            std::thread::spawn(move || {
                let result = crate::open_with(
                    path,
                    OpenOptions {
                        audio: hear,
                        previews: false,
                    },
                );
                if let Ok(mut guard) = writer.lock() {
                    *guard = Some(result);
                }
            });
            poll_open(slot, watch, ready.clone());
        });
        let shown = opened.clone();
        let stage_fill = fill;
        let controls = self.controls;
        element! {
            div(key = "video-open", class = Class::Column) {
                { if_else(
                    move || shown.with(|value| value.is_some()),
                    move || match shown.get() {
                        Some(Ok(source)) => Stage {
                            source,
                            fill: stage_fill,
                            controls,
                        }
                        .into_element(),
                        Some(Err(error)) => text(error).key("video-error"),
                        None => waiting_stage(),
                    },
                    || waiting_stage(),
                ) }
            }
        }
        .style(shell_style())
    }
}

fn poll_open(
    slot: Arc<Mutex<Option<Result<VideoSource, String>>>>,
    opened: Reactive<Option<Result<VideoSource, String>>>,
    ready: Listener<VideoSource>,
) {
    Context::current().timeout("video-open", Duration::from_millis(50), move || {
        let loaded = slot.lock().ok().and_then(|mut guard| guard.take());
        match loaded {
            Some(result) => {
                if let Ok(source) = &result {
                    ready.emit(source.clone());
                }
                opened.set(Some(result));
            }
            None => poll_open(slot, opened, ready),
        }
    });
}

#[derive(Clone)]
struct Stage {
    source: VideoSource,
    fill: Reactive<bool>,
    controls: Reactive<bool>,
}

impl Component for Stage {
    fn render(&self) -> Element {
        let source = self.source.clone();
        let pixels = Reactive::new((0_u32, 0_u32));
        let chrome = Chrome {
            source: source.clone(),
            fraction: Reactive::new(0.0_f32),
            label: reactive(String::from("0:00 / 0:00")),
            action: reactive(String::from(PAUSE)),
            playing: reactive(true),
            scrubbing: reactive(false),
            resume: reactive(true),
            settings: reactive(false),
            fill: self.fill,
            full_label: reactive(String::from(FULL)),
            over_chrome: reactive(false),
            cue: reactive(String::new()),
            volume: Reactive::new(1.0_f32),
            muted: reactive(false),
            vol_drag: reactive(false),
            pixels,
            qualities: source.qualities(),
            selected: reactive(source.selected_quality()),
            rate: Reactive::new(source.playback_rate()),
        };
        let over_stage = reactive(false);
        if self.controls.get() || !source.text_tracks().is_empty() {
            let clock = chrome.clone();
            on_mounted(move || {
                arm_spinner();
                arm_clock(clock.clone());
            });
        } else {
            on_mounted(arm_spinner);
        }
        let cue_shown = chrome.cue;
        let cue_text = chrome.cue;
        let windowed = chrome.clone();
        let covered = chrome.clone();
        let windowed_hover = over_stage;
        let covered_hover = over_stage;
        let windowed_controls = self.controls;
        let covered_controls = self.controls;
        let windowed_source = source.clone();
        let covered_source = source;
        let stage_fill = self.fill;
        div()
            .class(Class::Column)
            .style(shell_style())
            .child(if_else(
                move || stage_fill.get(),
                move || {
                    stage_view(
                        covered_source.clone(),
                        covered.clone(),
                        covered_controls,
                        covered_hover,
                        true,
                    )
                },
                move || {
                    stage_view(
                        windowed_source.clone(),
                        windowed.clone(),
                        windowed_controls,
                        windowed_hover,
                        false,
                    )
                },
            ))
            .child(show(
                move || !cue_shown.get().is_empty(),
                move || {
                    text_dyn(move || cue_text.get())
                        .class(Class::Muted)
                        .key("video-cue")
                },
            ))
    }
}

#[derive(Clone)]
struct Chrome {
    source: VideoSource,
    fraction: Reactive<f32>,
    label: Reactive<String>,
    action: Reactive<String>,
    playing: Reactive<bool>,
    scrubbing: Reactive<bool>,
    resume: Reactive<bool>,
    settings: Reactive<bool>,
    fill: Reactive<bool>,
    full_label: Reactive<String>,
    over_chrome: Reactive<bool>,
    cue: Reactive<String>,
    volume: Reactive<f32>,
    muted: Reactive<bool>,
    vol_drag: Reactive<bool>,
    pixels: Reactive<(u32, u32)>,
    qualities: Vec<VideoQuality>,
    selected: Reactive<Option<u32>>,
    rate: Reactive<f32>,
}

impl Chrome {
    fn chrome_visible(&self, over_stage: bool) -> bool {
        over_stage
            || self.over_chrome.get()
            || self.settings.get()
            || self.scrubbing.get()
            || self.vol_drag.get()
            || !self.playing.get()
    }
}

/// Mounts with each stage branch. Fullscreen swaps that branch and drops the timer on the
/// old node, so the new branch has to arm the clock again.
#[derive(Clone)]
struct ClockHost {
    chrome: Chrome,
}

impl Component for ClockHost {
    fn render(&self) -> Element {
        let clock = self.chrome.clone();
        on_mounted(move || arm_clock(clock.clone()));
        fragment(Vec::<Element>::new())
    }
}

fn stage_view(
    source: VideoSource,
    chrome: Chrome,
    controls: Reactive<bool>,
    over_stage: Reactive<bool>,
    fill: bool,
) -> Element {
    let picture = source;
    let bar_chrome = chrome.clone();
    let menu_chrome = chrome.clone();
    let hover_stage = over_stage;
    let mut stage = div()
        .key("video-stage")
        .class(Class::Column)
        .on_hover(move |inside| hover_stage.set(inside));
    if fill {
        stage = stage
            .overlay()
            .place(Point::new(0.0, 0.0))
            .style(fullscreen_frame());
    } else {
        let pixels = chrome.pixels;
        stage = stage.style_dyn(move || window_frame(pixels.get()));
    }
    let radius = if fill { 0.0 } else { 8.0 };
    stage
        .child(
            ClockHost {
                chrome: chrome.clone(),
            }
            .into_element(),
        )
        .child(
            video(picture).fit(ObjectFit::Contain).style(
                StyleRefinement::default()
                    .width(percent(100.0))
                    .min_height(0.0)
                    .flex_grow(1.0)
                    .background(Color::BLACK)
                    .radius(radius),
            ),
        )
        .child(show(
            {
                let waiting = chrome.pixels;
                move || waiting.get() == (0, 0)
            },
            || spinner().overlay().place(Dynamic(loading_point)),
        ))
        .child(if_else(
            move || controls.get(),
            move || {
                let bar = bar_chrome.clone();
                let menu = menu_chrome.clone();
                let bar_when = bar.clone();
                let stage_hover = over_stage;
                fragment([
                    show(
                        move || bar_when.chrome_visible(stage_hover.get()),
                        move || control_bar(bar.clone()),
                    ),
                    show(
                        {
                            let open = menu.settings;
                            move || open.get()
                        },
                        move || settings_menu(menu.clone()),
                    ),
                ])
            },
            || fragment(Vec::<Element>::new()),
        ))
}

fn control_bar(chrome: Chrome) -> Element {
    let hover = chrome.over_chrome;
    let play = chrome.clone();
    let seek = chrome.clone();
    let toggle_settings = chrome.settings;
    let toggle_fill = chrome.fill;
    let full_text = chrome.full_label;
    let action = chrome.action;
    let full_label = chrome.full_label;
    let label = chrome.label;
    let placed = chrome.pixels;
    let sized = chrome.pixels;
    div()
        .key("video-controls")
        .overlay()
        .on_hover(move |inside| hover.set(inside))
        .place(Dynamic(move || bar_origin(placed.get())))
        .style(bar_style())
        .style_dyn(move || bar_width(sized.get()))
        .child(progress(seek))
        .child(
            div()
                .style(
                    StyleRefinement::default()
                        .direction(FlexDirection::Row)
                        .align(Align::Center)
                        .gap(2.0)
                        .height(36.0)
                        .padding_x(6.0),
                )
                .child(
                    icon_button("video-play", move |_| toggle_play(&play))
                        .child(text_dyn(move || action.get())),
                )
                .child(volume_control(&chrome))
                .child(
                    text_dyn(move || label.get())
                        .key("video-clock")
                        .style(StyleRefinement::default().font_size(12.0).padding_x(8.0)),
                )
                .child(div().style(StyleRefinement::default().flex_grow(1.0)))
                .child(
                    icon_button("video-settings-toggle", move |_| {
                        toggle_settings.set(!toggle_settings.get());
                    })
                    .child(text(SETTINGS)),
                )
                .child(
                    icon_button("video-full", move |_| {
                        let next = !Context::current().is_fullscreen();
                        Context::current().request_fullscreen(next);
                        toggle_fill.set(next);
                        full_text.set(if next { FIT } else { FULL }.to_owned());
                    })
                    .child(text_dyn(move || full_label.get())),
                ),
        )
}

fn volume_control(chrome: &Chrome) -> Element {
    if !chrome.source.wants_audio() {
        return fragment(Vec::<Element>::new());
    }
    let toggle = chrome.clone();
    let slider = chrome.clone();
    let muted = chrome.muted;
    let volume = chrome.volume;
    let fill = chrome.volume;
    let fill_muted = chrome.muted;
    let knob = chrome.volume;
    let knob_muted = chrome.muted;
    div()
        .style(
            StyleRefinement::default()
                .direction(FlexDirection::Row)
                .align(Align::Center)
                .gap(2.0),
        )
        .child(
            icon_button("video-mute", move |_| toggle_mute(&toggle))
                .child(speaker_mark(muted, volume)),
        )
        .child(
            div()
                .key("video-volume")
                .style(
                    StyleRefinement::default()
                        .direction(FlexDirection::Column)
                        .justify(Justify::Center)
                        .width(72.0)
                        .height(32.0)
                        .cursor(CursorIcon::Pointer),
                )
                .on_pointer(move |event| {
                    volume_pointer(&slider, event.phase, event.button, event.position.x);
                })
                .child(
                    div()
                        .style(
                            StyleRefinement::default()
                                .width(percent(100.0))
                                .height(3.0)
                                .radius(2.0)
                                .overflow(Overflow::Hidden)
                                .background(Color::rgba(255, 255, 255, 80)),
                        )
                        .child(div().style_dyn(move || {
                            let shown = if fill_muted.get() { 0.0 } else { fill.get() };
                            StyleRefinement::default()
                                .width(percent(shown * 100.0))
                                .height(3.0)
                                .background(Color::WHITE)
                        })),
                )
                .child(
                    div()
                        .key("video-volume-knob")
                        .overlay()
                        .place(Dynamic(move || {
                            let shown = if knob_muted.get() { 0.0 } else { knob.get() };
                            knob_point("video-volume", shown, 10.0)
                        }))
                        .style(
                            StyleRefinement::default()
                                .width(10.0)
                                .height(10.0)
                                .radius(5.0)
                                .background(Color::WHITE)
                                .pointer_events(PointerEvents::None),
                        ),
                ),
        )
}

fn toggle_mute(chrome: &Chrome) {
    if chrome.muted.get() || chrome.source.muted() {
        let restore = chrome.volume.get().clamp(0.05, 1.0);
        chrome.volume.set(restore);
        chrome.source.set_volume(restore);
        chrome.source.set_muted(false);
        chrome.muted.set(false);
    } else {
        chrome.source.set_muted(true);
        chrome.muted.set(true);
    }
}

fn volume_pointer(chrome: &Chrome, phase: PointerPhase, button: PointerButton, x: f32) {
    if phase == PointerPhase::Down && button != PointerButton::Primary {
        return;
    }
    let Some(rect) = Context::current().node_rect("video-volume") else {
        return;
    };
    if rect.width <= 0.0 {
        return;
    }
    let level = ((x - rect.x) / rect.width).clamp(0.0, 1.0);
    match phase {
        PointerPhase::Down => {
            chrome.vol_drag.set(true);
            apply_volume(chrome, level);
        }
        PointerPhase::Move => {
            if !chrome.vol_drag.get() {
                return;
            }
            apply_volume(chrome, level);
        }
        PointerPhase::Up => {
            if !chrome.vol_drag.get() {
                return;
            }
            apply_volume(chrome, level);
            chrome.vol_drag.set(false);
        }
    }
}

fn apply_volume(chrome: &Chrome, level: f32) {
    let level = level.clamp(0.0, 1.0);
    let mute = level <= 0.001;
    chrome.source.set_volume(level);
    if mute != chrome.source.muted() {
        chrome.source.set_muted(mute);
    }
    if !mute {
        chrome.volume.set(level);
    }
    chrome.muted.set(mute);
}

fn speaker_mark(muted: Reactive<bool>, volume: Reactive<f32>) -> Element {
    let silent = muted;
    let quiet_muted = muted;
    let quiet_volume = volume;
    let loud_muted = muted;
    let loud_volume = volume;
    div()
        .style(
            StyleRefinement::default()
                .width(22.0)
                .height(22.0)
                .align(Align::Center)
                .justify(Justify::Center),
        )
        .child(show(
            move || silent.get() || volume.get() <= 0.001,
            || speaker_icon("speaker-mute.svg"),
        ))
        .child(show(
            move || {
                !quiet_muted.get() && {
                    let level = quiet_volume.get();
                    level > 0.001 && level < 0.5
                }
            },
            || speaker_icon("speaker-low.svg"),
        ))
        .child(show(
            move || !loud_muted.get() && loud_volume.get() >= 0.5,
            || speaker_icon("speaker-high.svg"),
        ))
}

fn speaker_icon(file: &str) -> Element {
    image(format!("{}/assets/{file}", env!("CARGO_MANIFEST_DIR")))
        .style(StyleRefinement::default().width(22.0).height(22.0))
}

fn toggle_play(chrome: &Chrome) {
    if chrome.scrubbing.get() {
        let next = !chrome.resume.get();
        chrome.resume.set(next);
        chrome
            .action
            .set(if next { PAUSE } else { PLAY }.to_owned());
        return;
    }
    if chrome.source.playing() {
        chrome.source.pause();
        chrome.playing.set(false);
        chrome.action.set(PLAY.to_owned());
    } else {
        chrome.source.play();
        chrome.playing.set(true);
        chrome.action.set(PAUSE.to_owned());
    }
}

fn progress(chrome: Chrome) -> Element {
    let width = chrome.fraction;
    let scrub = chrome.clone();
    let knob = chrome.fraction;
    div()
        .key("video-progress")
        .style(progress_style())
        .on_pointer(move |event| scrub_pointer(&scrub, event.phase, event.button, event.position.x))
        .child(
            div()
                .style(
                    StyleRefinement::default()
                        .width(percent(100.0))
                        .height(3.0)
                        .radius(2.0)
                        .overflow(Overflow::Hidden)
                        .background(Color::rgba(255, 255, 255, 70)),
                )
                .child(div().style_dyn(move || {
                    StyleRefinement::default()
                        .width(percent(width.get()))
                        .height(3.0)
                        .background(RED)
                })),
        )
        .child(
            div()
                .key("video-playhead")
                .overlay()
                .place(Dynamic(move || {
                    knob_point("video-progress", knob.get() / 100.0, 12.0)
                }))
                .style(
                    StyleRefinement::default()
                        .width(12.0)
                        .height(12.0)
                        .radius(6.0)
                        .background(RED)
                        .pointer_events(PointerEvents::None),
                ),
        )
}

fn scrub_pointer(chrome: &Chrome, phase: PointerPhase, button: PointerButton, x: f32) {
    if phase == PointerPhase::Down && button != PointerButton::Primary {
        return;
    }
    let Some(rect) = Context::current().node_rect("video-progress") else {
        return;
    };
    let pts = scrub_pts(x, rect.x, rect.width, chrome.source.duration());
    match phase {
        PointerPhase::Down => {
            chrome.resume.set(chrome.source.playing());
            chrome.scrubbing.set(true);
            chrome.source.pause();
            chrome.playing.set(false);
            chrome.source.present_at(pts);
            show_at(chrome, pts);
        }
        PointerPhase::Move => {
            if !chrome.scrubbing.get() {
                return;
            }
            chrome.source.present_at(pts);
            show_at(chrome, pts);
        }
        PointerPhase::Up => {
            if !chrome.scrubbing.get() {
                return;
            }
            chrome.source.seek(pts);
            show_at(chrome, pts);
            chrome.scrubbing.set(false);
            if chrome.resume.get() {
                chrome.source.play();
                chrome.playing.set(true);
                chrome.action.set(PAUSE.to_owned());
            } else {
                chrome.playing.set(false);
                chrome.action.set(PLAY.to_owned());
            }
        }
    }
}

fn show_at(chrome: &Chrome, at: Duration) {
    chrome
        .fraction
        .set(played_percent(at, chrome.source.duration()));
    chrome.label.set(clock_text(at, chrome.source.duration()));
}

fn settings_menu(chrome: Chrome) -> Element {
    let hover = chrome.over_chrome;
    let rows = menu_rows(chrome.qualities.len());
    let mut menu = div()
        .key("video-settings")
        .overlay()
        .on_hover(move |inside| hover.set(inside))
        .on_dismiss({
            let settings = chrome.settings;
            move |_| settings.set(false)
        })
        .place(Dynamic({
            let pixels = chrome.pixels;
            move || menu_origin(rows, pixels.get())
        }))
        .style(menu_style())
        .style_dyn({
            let pixels = chrome.pixels;
            move || menu_width(pixels.get())
        });
    if !chrome.qualities.is_empty() {
        menu = menu.child(menu_heading("Quality"));
        menu = menu.child(quality_row(chrome.clone(), None, "Auto".to_owned()));
        for quality in chrome.qualities.clone() {
            let edge = quality.edge;
            menu = menu.child(quality_row(chrome.clone(), Some(edge), quality.label));
        }
    }
    menu = menu.child(menu_heading("Speed"));
    for speed in SPEEDS {
        menu = menu.child(speed_row(chrome.clone(), speed));
    }
    menu
}

fn quality_row(chrome: Chrome, edge: Option<u32>, label: String) -> Element {
    let chosen = chrome.selected;
    let pick = chrome.clone();
    menu_row(
        label,
        move || chosen.get() == edge,
        move |_| {
            pick.source.select_quality(edge);
            pick.selected.set(edge);
            pick.settings.set(false);
            pick.source.seek(pick.source.presented());
        },
    )
}

fn speed_row(chrome: Chrome, speed: f32) -> Element {
    let chosen = chrome.rate;
    let pick = chrome.clone();
    menu_row(
        speed_label(speed),
        move || (chosen.get() - speed).abs() < 0.01,
        move |_| {
            pick.source.set_playback_rate(speed);
            pick.rate.set(speed);
            pick.settings.set(false);
        },
    )
}

fn menu_row(
    label: String,
    chosen: impl Fn() -> bool + 'static,
    on_click: impl FnMut(&cappy_core::ClickEvent) + 'static,
) -> Element {
    div()
        .style(
            StyleRefinement::default()
                .height(MENU_ROW)
                .padding_x(10.0)
                .align(Align::Center)
                .cursor(CursorIcon::Pointer)
                .radius(4.0),
        )
        .style_dyn(move || {
            let background = if chosen() {
                Color::rgba(255, 255, 255, 48)
            } else {
                Color::TRANSPARENT
            };
            StyleRefinement::default().background(background)
        })
        .on_click(on_click)
        .child(text(label))
}

fn menu_heading(label: &str) -> Element {
    text(label.to_owned()).style(
        StyleRefinement::default()
            .height(MENU_ROW)
            .padding_x(10.0)
            .font_size(12.0)
            .text_color(Color::rgba(255, 255, 255, 170)),
    )
}

fn icon_button(key: &str, on_click: impl FnMut(&cappy_core::ClickEvent) + 'static) -> Element {
    div()
        .key(key.to_owned())
        .style(
            StyleRefinement::default()
                .height(32.0)
                .padding_x(6.0)
                .align(Align::Center)
                .justify(Justify::Center)
                .font_size(15.0)
                .cursor(CursorIcon::Pointer),
        )
        .on_click(on_click)
}

fn knob_point(key: &str, fraction: f32, diameter: f32) -> Point {
    let Some(rect) = Context::current().node_rect(key) else {
        return Point::new(-40.0, -40.0);
    };
    let travel = (rect.width - diameter).max(0.0);
    Point::new(
        rect.x + travel * fraction.clamp(0.0, 1.0),
        rect.y + (rect.height - diameter) * 0.5,
    )
}

fn arm_clock(chrome: Chrome) {
    let follow = chrome.clone();
    Context::current().timeout("video-stage", Duration::from_millis(200), move || {
        let duration = follow.source.duration();
        let now = follow.source.presented();
        if !follow.scrubbing.get() {
            follow.label.set(clock_text(now, duration));
            follow.fraction.set(played_percent(now, duration));
            let playing = follow.source.playing();
            follow.playing.set(playing);
            follow
                .action
                .set(if playing { PAUSE } else { PLAY }.to_owned());
            if !follow.vol_drag.get() {
                follow.muted.set(follow.source.muted());
            }
            let fullscreen = Context::current().is_fullscreen();
            if follow.fill.get() != fullscreen {
                follow.fill.set(fullscreen);
                follow
                    .full_label
                    .set(if fullscreen { FIT } else { FULL }.to_owned());
            }
            if let Some(size) = follow.source.pixel_size()
                && follow.pixels.get() != size
            {
                follow.pixels.set(size);
            }
        }
        follow
            .cue
            .set(follow.source.cue_at(now).unwrap_or_default());
        arm_clock(follow);
    });
}

fn played_percent(at: Duration, duration: Duration) -> f32 {
    let span = duration.as_secs_f32();
    if span <= 0.0 {
        0.0
    } else {
        (at.as_secs_f32() / span * 100.0).clamp(0.0, 100.0)
    }
}

fn clock_text(now: Duration, duration: Duration) -> String {
    format!("{} / {}", stamp(now), stamp(duration))
}

fn stamp(time: Duration) -> String {
    let total = time.as_secs();
    let minutes = total / 60;
    let seconds = total % 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds)
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn speed_label(rate: f32) -> String {
    if (rate - 1.0).abs() < 0.01 {
        "Normal".to_owned()
    } else {
        let text = format!("{rate:.2}");
        let trimmed = text.trim_end_matches('0').trim_end_matches('.');
        format!("{trimmed}x")
    }
}

fn menu_rows(qualities: usize) -> f32 {
    let mut rows = 1 + SPEEDS.len();
    if qualities > 0 {
        rows += 2 + qualities;
    }
    MENU_PAD * 2.0 + rows as f32 * MENU_ROW
}

fn picture_rect(pixels: (u32, u32)) -> Rect {
    let Some(stage) = Context::current().node_rect("video-stage") else {
        return Rect::ZERO;
    };
    let (width, height) = pixels;
    if width == 0 || height == 0 || stage.width <= 0.0 || stage.height <= 0.0 {
        return stage;
    }
    let media = width as f32 / height as f32;
    let frame = stage.width / stage.height;
    if frame > media {
        let fitted = stage.height * media;
        Rect::new(
            stage.x + (stage.width - fitted) * 0.5,
            stage.y,
            fitted,
            stage.height,
        )
    } else {
        let fitted = stage.width / media;
        Rect::new(
            stage.x,
            stage.y + (stage.height - fitted) * 0.5,
            stage.width,
            fitted,
        )
    }
}

fn bar_origin(pixels: (u32, u32)) -> Point {
    let picture = picture_rect(pixels);
    Point::new(
        picture.x,
        (picture.y + picture.height - BAR_HEIGHT).max(picture.y),
    )
}

fn menu_origin(height: f32, pixels: (u32, u32)) -> Point {
    let picture = picture_rect(pixels);
    let width = picture.width.min(MENU_WIDTH);
    Point::new(
        picture.x + picture.width - width,
        (picture.y + picture.height - BAR_HEIGHT - height).max(picture.y),
    )
}

fn bar_width(pixels: (u32, u32)) -> StyleRefinement {
    StyleRefinement::default().width(picture_rect(pixels).width)
}

fn menu_width(pixels: (u32, u32)) -> StyleRefinement {
    StyleRefinement::default().width(picture_rect(pixels).width.min(MENU_WIDTH))
}

fn progress_style() -> StyleRefinement {
    StyleRefinement::default()
        .direction(FlexDirection::Column)
        .justify(Justify::Center)
        .align(Align::Stretch)
        .width(percent(100.0))
        .height(12.0)
        .overflow(Overflow::Hidden)
        .cursor(CursorIcon::Pointer)
}

fn bar_style() -> StyleRefinement {
    StyleRefinement::default()
        .direction(FlexDirection::Column)
        .height(BAR_HEIGHT)
        .overflow(Overflow::Hidden)
        .radius_bottom_left(8.0)
        .radius_bottom_right(8.0)
        .background(Color::rgba(0, 0, 0, 168))
        .text_color(Color::WHITE)
        .font_size(13.0)
}

fn menu_style() -> StyleRefinement {
    StyleRefinement::default()
        .direction(FlexDirection::Column)
        .padding(MENU_PAD)
        .radius(8.0)
        .background(Color::rgba(20, 20, 20, 230))
        .text_color(Color::WHITE)
        .font_size(13.0)
}

fn waiting_stage() -> Element {
    div()
        .key("video-waiting")
        .style(
            window_frame((0, 0))
                .align(Align::Center)
                .justify(Justify::Center),
        )
        .child(spinner())
}

fn spinner() -> Element {
    let mut wheel = div().key("video-spinner").style(
        StyleRefinement::default()
            .width(SPINNER)
            .height(SPINNER)
            .direction(FlexDirection::Column)
            .pointer_events(PointerEvents::None),
    );
    for index in 0..SPINNER_DOTS {
        let mut spoke = StyleRefinement::default()
            .width(SPINNER)
            .height(SPINNER)
            .direction(FlexDirection::Column)
            .align(Align::Center)
            .justify(Justify::Start)
            .pointer_events(PointerEvents::None);
        if index > 0 {
            spoke = spoke.margin_top(-SPINNER);
        }
        let shade = 0.15 + 0.85 * (index as f32 / (SPINNER_DOTS - 1) as f32);
        let alpha = (shade * 255.0).round() as u8;
        wheel = wheel.child(
            div()
                .key(format!("video-spinner-{index}"))
                .style(spoke)
                .child(
                    div().style(
                        StyleRefinement::default()
                            .width(SPINNER_DOT)
                            .height(SPINNER_DOT)
                            .radius(SPINNER_DOT / 2.0)
                            .background(Color::rgba(255, 255, 255, alpha))
                            .pointer_events(PointerEvents::None),
                    ),
                ),
        );
    }
    wheel
}

fn arm_spinner() {
    Context::current().animate(|motion| {
        let Some(id) = motion.find_by_key("video-spinner") else {
            return;
        };
        motion.animate(
            id,
            Motion::rotation(360.0)
                .duration(Duration::from_millis(900))
                .ease(Ease::Linear)
                .looped(),
        );
        let step = SPINNER_SWEEP / (SPINNER_DOTS - 1) as f32;
        for index in 0..SPINNER_DOTS {
            let Some(spoke) = motion.find_by_key(&format!("video-spinner-{index}")) else {
                continue;
            };
            motion.animate(
                spoke,
                Motion::rotation(step * index as f32)
                    .duration(Duration::ZERO)
                    .ease(Ease::Linear),
            );
        }
    });
}

fn loading_point() -> Point {
    let Some(rect) = Context::current().node_rect("video-stage") else {
        return Point::new(-80.0, -80.0);
    };
    Point::new(
        rect.x + (rect.width - SPINNER) * 0.5,
        rect.y + (rect.height - SPINNER) * 0.5,
    )
}

fn shell_style() -> StyleRefinement {
    StyleRefinement::default().width(480.0).flex_grow(0.0)
}

fn window_frame(pixels: (u32, u32)) -> StyleRefinement {
    StyleRefinement::default()
        .width(percent(100.0))
        .height(fitted_height(pixels))
        .flex_grow(0.0)
        .background(Color::BLACK)
        .overflow(Overflow::Hidden)
        .radius(8.0)
}

fn fullscreen_frame() -> StyleRefinement {
    StyleRefinement::default()
        .width(percent(100.0))
        .height(percent(100.0))
        .background(Color::BLACK)
        .overflow(Overflow::Hidden)
}

fn fitted_height(pixels: (u32, u32)) -> f32 {
    let (width, height) = pixels;
    if width == 0 || height == 0 {
        270.0
    } else {
        480.0 * height as f32 / width as f32
    }
}
