//! Duration tweens for translation and opacity.
//!
//! A tween owns one property: a destination and a duration, sampled from the value on screen.
//! `class_dyn` and `style_dyn` do not read or write it. When the clock finishes, the property
//! stays on the destination. Hiding or disposing the node clears it.

use std::time::{Duration, Instant};

use cappy_layout::{Point, Rect};

use super::{NodeId, Ui};
use crate::paint::paint_bounds;

const PI: f32 = std::f32::consts::PI;

/// Maps a linear clock `t` in `0..=1` onto the eased unit interval.
///
/// `InCubic` and `OutCubic` are Penner polynomials. They are not the CSS curves
/// `ease-in` and `ease-out`; those are [`Ease::CubicBezier`] values.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Ease {
    Linear,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    #[default]
    OutCubic,
    InOutCubic,
    InSine,
    OutSine,
    InOutSine,
    InExpo,
    OutExpo,
    InOutExpo,
    InBack,
    OutBack,
    InOutBack,
    OutBounce,
    /// CSS `cubic-bezier(x1, y1, x2, y2)`. Control points are in the unit square.
    CubicBezier {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
}

impl Ease {
    /// CSS `ease`.
    pub const fn css_ease() -> Self {
        Self::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        }
    }

    /// CSS `ease-in`.
    pub const fn css_ease_in() -> Self {
        Self::CubicBezier {
            x1: 0.42,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        }
    }

    /// CSS `ease-out`.
    pub const fn css_ease_out() -> Self {
        Self::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 0.58,
            y2: 1.0,
        }
    }

    /// CSS `ease-in-out`.
    pub const fn css_ease_in_out() -> Self {
        Self::CubicBezier {
            x1: 0.42,
            y1: 0.0,
            x2: 0.58,
            y2: 1.0,
        }
    }

    /// Eased progress. `t` is clamped to `0..=1`. Endpoints stay 0 and 1.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::InQuad => t * t,
            Self::OutQuad => t * (2.0 - t),
            Self::InOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let u = t * 2.0 - 1.0;
                    -0.5 * (u * (u - 2.0) - 1.0)
                }
            }
            Self::InCubic => t * t * t,
            Self::OutCubic => {
                let u = t - 1.0;
                u * u * u + 1.0
            }
            Self::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = 2.0 * t - 2.0;
                    0.5 * u * u * u + 1.0
                }
            }
            Self::InSine => 1.0 - (t * PI / 2.0).cos(),
            Self::OutSine => (t * PI / 2.0).sin(),
            Self::InOutSine => 0.5 * (1.0 - (t * PI).cos()),
            Self::InExpo => {
                if t == 0.0 {
                    0.0
                } else {
                    2.0_f32.powf(10.0 * t - 10.0)
                }
            }
            Self::OutExpo => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f32.powf(-10.0 * t)
                }
            }
            Self::InOutExpo => {
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else if t < 0.5 {
                    2.0_f32.powf(20.0 * t - 10.0) / 2.0
                } else {
                    (2.0 - 2.0_f32.powf(-20.0 * t + 10.0)) / 2.0
                }
            }
            Self::InBack => in_back(t),
            Self::OutBack => out_back(t),
            Self::InOutBack => in_out_back(t),
            Self::OutBounce => out_bounce(t),
            Self::CubicBezier { x1, y1, x2, y2 } => {
                let solved = solve_bezier_x(t, x1, x2);
                bezier(solved, y1, y2)
            }
        }
    }
}

/// One destination for a single property. The driver samples it from the value on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    kind: MotionKind,
    duration: Duration,
    ease: Ease,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MotionKind {
    Translation(Point),
    Opacity(f32),
}

impl Motion {
    /// Slide to `(x, y)` logical pixels from the translation on screen. Default ease is
    /// [`Ease::OutCubic`]. Duration starts at zero, which finishes on the next sample.
    pub fn translation(x: f32, y: f32) -> Self {
        Self {
            kind: MotionKind::Translation(Point::new(x, y)),
            duration: Duration::ZERO,
            ease: Ease::OutCubic,
        }
    }

    /// Fade to `opacity` from the opacity on screen.
    pub fn opacity(opacity: f32) -> Self {
        Self {
            kind: MotionKind::Opacity(opacity),
            duration: Duration::ZERO,
            ease: Ease::OutCubic,
        }
    }

    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    #[must_use]
    pub fn ease(mut self, ease: Ease) -> Self {
        self.ease = ease;
        self
    }
}

/// What [`Context::animate`](crate::Context::animate) can see. The closure runs inside
/// [`Ui::update`](Ui::update), after layout, not at the call.
pub struct MotionCtx<'a> {
    ui: &'a mut Ui,
}

impl<'a> MotionCtx<'a> {
    pub(super) fn new(ui: &'a mut Ui) -> Self {
        Self { ui }
    }

    pub fn find_by_key(&self, key: &str) -> Option<NodeId> {
        self.ui.find_by_key(key)
    }

    pub fn node_rect(&self, id: NodeId) -> Option<Rect> {
        self.ui.node_rect(id)
    }

    pub fn animate(&mut self, id: NodeId, motion: Motion) {
        self.ui.animate(id, motion);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tween<T> {
    pub(crate) from: T,
    pub(crate) to: T,
    /// Filled on the first sample so tests can drive the clock through [`Ui::update_at`].
    pub(crate) start: Option<Instant>,
    pub(crate) duration: Duration,
    pub(crate) ease: Ease,
}

impl<T: Copy> Tween<T> {
    fn sample(self, now: Instant, lerp: impl Fn(T, T, f32) -> T) -> (T, bool) {
        let Some(start) = self.start else {
            return (self.from, false);
        };
        let t = unit_progress(start, now, self.duration);
        (lerp(self.from, self.to, self.ease.apply(t)), t >= 1.0)
    }
}

pub(crate) fn unit_progress(start: Instant, now: Instant, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    let elapsed = now.saturating_duration_since(start).as_secs_f32();
    (elapsed / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn lerp_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn lerp_point(from: Point, to: Point, t: f32) -> Point {
    Point::new(lerp_f32(from.x, to.x, t), lerp_f32(from.y, to.y, t))
}

impl Ui {
    /// Replaces the tween for this motion's property. `from` is the value on screen and the
    /// duration clock starts at the next sample.
    pub fn animate(&mut self, id: NodeId, motion: Motion) {
        if !self.is_current(id) {
            return;
        }
        let node = &mut self.nodes[id.index()];
        match motion.kind {
            MotionKind::Translation(to) => {
                node.translation_tween = Some(Tween {
                    from: node.translation,
                    to,
                    start: None,
                    duration: motion.duration,
                    ease: motion.ease,
                });
            }
            MotionKind::Opacity(to) => {
                node.opacity_tween = Some(Tween {
                    from: node.opacity,
                    to,
                    start: None,
                    duration: motion.duration,
                    ease: motion.ease,
                });
            }
        }
    }

    pub(super) fn motion_animating(&self) -> bool {
        self.nodes.iter().any(|node| {
            node.alive && (node.translation_tween.is_some() || node.opacity_tween.is_some())
        })
    }

    pub(super) fn drain_motions(&mut self) {
        loop {
            let jobs = self.queue.runtime.take_motions();
            if jobs.is_empty() {
                break;
            }
            for job in jobs {
                job(self);
            }
        }
    }

    /// Samples running tweens. A finished tween drops its clock and leaves the property on the
    /// destination.
    pub(super) fn step_motion(&mut self, now: Instant) {
        let mut moved = false;
        let mut faded = Vec::new();
        for index in 0..self.nodes.len() {
            if !self.nodes[index].alive {
                continue;
            }
            let previous_translation = self.nodes[index].translation;
            if let Some(mut tween) = self.nodes[index].translation_tween {
                if tween.start.is_none() {
                    tween.start = Some(now);
                }
                let (value, done) = tween.sample(now, lerp_point);
                if done {
                    self.nodes[index].translation_tween = None;
                    self.nodes[index].translation = tween.to;
                } else {
                    self.nodes[index].translation = value;
                    self.nodes[index].translation_tween = Some(tween);
                }
                if self.nodes[index].translation != previous_translation {
                    moved = true;
                }
            }
            let previous_opacity = self.nodes[index].opacity;
            if let Some(mut tween) = self.nodes[index].opacity_tween {
                if tween.start.is_none() {
                    tween.start = Some(now);
                }
                let (value, done) = tween.sample(now, lerp_f32);
                if done {
                    self.nodes[index].opacity_tween = None;
                    self.nodes[index].opacity = tween.to;
                } else {
                    self.nodes[index].opacity = value;
                    self.nodes[index].opacity_tween = Some(tween);
                }
                if self.nodes[index].opacity != previous_opacity {
                    faded.push(index);
                }
            }
        }
        if moved {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
        for index in faded {
            let id = NodeId::new(index as u32, self.nodes[index].generation);
            self.damage_opacity(id);
        }
    }

    fn damage_opacity(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let (painted, bounds, children) = {
                let node = &self.nodes[current.index()];
                (
                    node.painted,
                    paint_bounds(node.rect, node.style.visual.shadow),
                    node.children.clone(),
                )
            };
            self.damage.add(painted);
            self.damage.add(bounds);
            stack.extend(children);
        }
        self.display_list_stale = true;
    }

    pub(super) fn cancel_motion(&mut self, id: NodeId) {
        if !self.is_current(id) {
            return;
        }
        let node = &mut self.nodes[id.index()];
        let moved = node.translation != Point::default() || node.translation_tween.is_some();
        let faded = node.opacity != 1.0 || node.opacity_tween.is_some();
        node.translation_tween = None;
        node.opacity_tween = None;
        node.translation = Point::default();
        node.opacity = 1.0;
        if moved {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
        if faded {
            node.paint_dirty = true;
            self.paint_dirty = true;
        }
    }
}

const BACK: f32 = 1.70158;

fn in_back(t: f32) -> f32 {
    t * t * ((BACK + 1.0) * t - BACK)
}

fn out_back(t: f32) -> f32 {
    let u = t - 1.0;
    u * u * ((BACK + 1.0) * u + BACK) + 1.0
}

fn in_out_back(t: f32) -> f32 {
    let c = BACK * 1.525;
    if t < 0.5 {
        let u = t * 2.0;
        0.5 * (u * u * ((c + 1.0) * u - c))
    } else {
        let u = t * 2.0 - 2.0;
        0.5 * (u * u * ((c + 1.0) * u + c) + 2.0)
    }
}

fn out_bounce(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let u = t - 1.5 / D;
        N * u * u + 0.75
    } else if t < 2.5 / D {
        let u = t - 2.25 / D;
        N * u * u + 0.9375
    } else {
        let u = t - 2.625 / D;
        N * u * u + 0.984375
    }
}

/// Cubic bezier with endpoints `(0, 0)` and `(1, 1)`.
fn bezier(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

fn bezier_derivative(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// Unit-curve parameter whose x equals `x`. Newton, then bisection.
fn solve_bezier_x(x: f32, x1: f32, x2: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let mut t = x;
    for _ in 0..8 {
        let error = bezier(t, x1, x2) - x;
        if error.abs() < 1.0e-6 {
            return t.clamp(0.0, 1.0);
        }
        let slope = bezier_derivative(t, x1, x2);
        if slope.abs() < 1.0e-6 {
            break;
        }
        t = (t - error / slope).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    t = x;
    for _ in 0..8 {
        let at = bezier(t, x1, x2);
        if (at - x).abs() < 1.0e-6 {
            break;
        }
        if at < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) * 0.5;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoints(ease: Ease) {
        let start = ease.apply(0.0);
        let end = ease.apply(1.0);
        assert!((start - 0.0).abs() < 1.0e-5, "{ease:?} starts at {start}");
        assert!((end - 1.0).abs() < 1.0e-5, "{ease:?} ends at {end}");
    }

    #[test]
    fn every_ease_holds_the_endpoints() {
        let named = [
            Ease::Linear,
            Ease::InQuad,
            Ease::OutQuad,
            Ease::InOutQuad,
            Ease::InCubic,
            Ease::OutCubic,
            Ease::InOutCubic,
            Ease::InSine,
            Ease::OutSine,
            Ease::InOutSine,
            Ease::InExpo,
            Ease::OutExpo,
            Ease::InOutExpo,
            Ease::InBack,
            Ease::OutBack,
            Ease::InOutBack,
            Ease::OutBounce,
            Ease::css_ease(),
            Ease::css_ease_in(),
            Ease::css_ease_out(),
            Ease::css_ease_in_out(),
        ];
        for ease in named {
            endpoints(ease);
        }
    }

    #[test]
    fn out_cubic_leads_linear_then_settles() {
        let mid = Ease::OutCubic.apply(0.5);
        assert!(mid > 0.5);
        assert!(mid < 1.0);
    }

    #[test]
    fn css_ease_in_lags_behind_linear() {
        let mid = Ease::css_ease_in().apply(0.5);
        assert!(mid < 0.5);
        assert!(mid > 0.0);
    }

    #[test]
    fn a_linear_bezier_is_the_clock() {
        let ease = Ease::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        assert!((ease.apply(0.3) - 0.3).abs() < 1.0e-4);
    }
}
