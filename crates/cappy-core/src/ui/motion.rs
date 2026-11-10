//! Duration tweens for translation, opacity, fill color, scale and rotation.
//!
//! A tween owns one property: a destination and a duration, sampled from the value on screen.
//! `class_dyn` and `style_dyn` do not read or write it. When the clock finishes, the property
//! stays on the destination. [`Repeat::Loop`] and [`Repeat::PingPong`] keep the clock.
//! [`Motion::delay`] waits before the first cycle. [`Anim`] runs clips one after another or at
//! the same time. Hiding or disposing the node clears it.

use crate::time::{Duration, Instant};

use cappy_layout::{Point, Rect};
use cappy_style::{Background, Color};

use super::{NodeId, Ui};

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

/// What happens when the clock reaches the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    /// Hold the destination and drop the clock.
    #[default]
    Once,
    /// Start again from the value captured at the beginning. A full turn (`0` to `360`) is seamless.
    Loop,
    /// Go to the destination, then back to the start. Each direction is its own cycle:
    /// the ease runs from 0 to 1 over `duration`, then again with the ends swapped.
    PingPong,
}

/// One destination for a single property. The driver samples it from the value on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    kind: MotionKind,
    duration: Duration,
    delay: Duration,
    ease: Ease,
    repeat: Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MotionKind {
    Translation(Point),
    Opacity(f32),
    Background(Color),
    Scale(f32),
    /// Clockwise degrees.
    Rotation(f32),
    /// Horizontal sine around the translation on screen. Ease is ignored.
    Shake(f32),
}

impl Motion {
    /// Slide to `(x, y)` logical pixels from the translation on screen. Default ease is
    /// [`Ease::OutCubic`]. Duration starts at zero, which finishes on the next sample.
    pub fn translation(x: f32, y: f32) -> Self {
        Self::new(MotionKind::Translation(Point::new(x, y)))
    }

    /// Fade to `opacity` from the opacity on screen.
    pub fn opacity(opacity: f32) -> Self {
        Self::new(MotionKind::Opacity(opacity))
    }

    /// Crossfade the solid fill to `color`. A gradient uses its first stop as the start.
    pub fn background(color: Color) -> Self {
        Self::new(MotionKind::Background(color))
    }

    /// Grow or shrink around the node center. `1` is the layout size. The layout box stays put.
    pub fn scale(scale: f32) -> Self {
        Self::new(MotionKind::Scale(scale))
    }

    /// Spin clockwise to `degrees`. [`Repeat::Loop`] from `0` to `360` repeats a full turn.
    pub fn rotation(degrees: f32) -> Self {
        Self::new(MotionKind::Rotation(degrees))
    }

    /// Oscillate horizontally by `amplitude` logical pixels around the translation on screen.
    /// `duration` is one cycle. The wave is a sine, so [`Motion::ease`] does not change it.
    pub fn shake(amplitude: f32) -> Self {
        Self::new(MotionKind::Shake(amplitude))
    }

    /// Slide to `(x, y)`. Same channel as [`Motion::translation`]. [`Motion::ping_pong`] flies
    /// there and back.
    pub fn fly(x: f32, y: f32) -> Self {
        Self::translation(x, y)
    }

    fn new(kind: MotionKind) -> Self {
        Self {
            kind,
            duration: Duration::ZERO,
            delay: Duration::ZERO,
            ease: Ease::OutCubic,
            repeat: Repeat::Once,
        }
    }

    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Wait before the first cycle. Repeats do not wait again.
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    #[must_use]
    pub fn ease(mut self, ease: Ease) -> Self {
        self.ease = ease;
        self
    }

    /// Repeat from the captured start. The clock does not stop.
    #[must_use]
    pub fn looped(mut self) -> Self {
        self.repeat = Repeat::Loop;
        self
    }

    /// Run to the destination, then back to the start, and keep alternating.
    /// Each direction replays [`Motion::ease`] over [`Motion::duration`].
    #[must_use]
    pub fn ping_pong(mut self) -> Self {
        self.repeat = Repeat::PingPong;
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

    /// Runs `anim` and returns a handle for [`MotionCtx::stop`]. A sequence step starts when the
    /// previous step finishes, and reads the value on screen at that moment.
    /// [`Anim::together`] starts every child at once.
    pub fn play(&mut self, anim: Anim) -> AnimHandle {
        self.ui.play(anim)
    }

    /// Stops the group from [`MotionCtx::play`]. Properties stay where they are. Later sequence
    /// steps do not run. A handle that already finished is ignored.
    pub fn stop(&mut self, handle: AnimHandle) {
        self.ui.stop(handle);
    }
}

/// Identifies one [`MotionCtx::play`] group. [`AnimHandle::stop`] does not need a [`Ui`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimHandle {
    id: u64,
    runtime_index: u32,
}

impl AnimHandle {
    /// Stops this group on the next update. The value on screen stays put, and later sequence
    /// steps do not run. Safe to call where no [`Ui`] is in scope. A finished handle is ignored.
    pub fn stop(self) {
        let runtime = crate::reactive::runtime_by_index(self.runtime_index);
        runtime.push_motion(Box::new(move |ui| ui.stop(self)));
    }
}

/// Motions grouped to run one after another or at the same time.
pub struct Anim {
    clip: Clip,
}

#[derive(Clone)]
pub(super) enum Clip {
    Play { id: NodeId, motion: Motion },
    Together(Vec<Clip>),
    Sequence(Vec<Clip>),
}

impl Anim {
    pub fn play(id: NodeId, motion: Motion) -> Self {
        Self {
            clip: Clip::Play { id, motion },
        }
    }

    /// Starts every child at the same moment. The group finishes when every finite child does.
    /// A child that loops does not finish, so a sequence step after this group does not start.
    pub fn together(parts: impl IntoIterator<Item = Anim>) -> Self {
        Self {
            clip: Clip::Together(parts.into_iter().map(|part| part.clip).collect()),
        }
    }

    /// Starts each child when the previous one finishes.
    pub fn sequence(parts: impl IntoIterator<Item = Anim>) -> Self {
        Self {
            clip: Clip::Sequence(parts.into_iter().map(|part| part.clip).collect()),
        }
    }
}

pub(super) enum Active {
    Play {
        id: NodeId,
        motion: Motion,
        owner: u64,
        started: bool,
    },
    Together(Vec<Active>),
    Sequence {
        pending: Vec<Clip>,
        current: Option<Box<Active>>,
    },
}

impl Active {
    fn from_clip(clip: Clip) -> Self {
        match clip {
            Clip::Play { id, motion } => Self::Play {
                id,
                motion,
                owner: 0,
                started: false,
            },
            Clip::Together(children) => {
                Self::Together(children.into_iter().map(Self::from_clip).collect())
            }
            Clip::Sequence(pending) => Self::Sequence {
                pending,
                current: None,
            },
        }
    }

    fn mentions(&self, id: NodeId) -> bool {
        match self {
            Self::Play { id: node, .. } => *node == id,
            Self::Together(children) => children.iter().any(|child| child.mentions(id)),
            Self::Sequence { pending, current } => {
                pending.iter().any(|clip| clip.mentions(id))
                    || current.as_ref().is_some_and(|child| child.mentions(id))
            }
        }
    }
}

pub(super) struct Running {
    id: u64,
    active: Active,
}

impl Clip {
    fn mentions(&self, id: NodeId) -> bool {
        match self {
            Self::Play { id: node, .. } => *node == id,
            Self::Together(children) | Self::Sequence(children) => {
                children.iter().any(|clip| clip.mentions(id))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tween<T> {
    pub(crate) from: T,
    pub(crate) to: T,
    /// Filled on the first sample so tests can drive the clock through [`Ui::update_at`].
    pub(crate) start: Option<Instant>,
    pub(crate) duration: Duration,
    pub(crate) delay: Duration,
    pub(crate) ease: Ease,
    pub(crate) repeat: Repeat,
    /// Set when an [`Anim`] started this tween, so the group can tell it is still the owner.
    owner: Option<u64>,
}

/// A sine on the translation channel. `duration` is one cycle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wave {
    pub(crate) amplitude: f32,
    pub(crate) base: Point,
    pub(crate) start: Option<Instant>,
    pub(crate) duration: Duration,
    pub(crate) delay: Duration,
    pub(crate) repeat: Repeat,
    owner: Option<u64>,
}

/// Seconds after `delay`. `None` while the delay is still running.
fn motion_elapsed(start: Instant, now: Instant, delay: Duration) -> Option<f32> {
    let elapsed = now.saturating_duration_since(start);
    if elapsed < delay {
        return None;
    }
    Some((elapsed - delay).as_secs_f32())
}

/// Linear clock position and whether a [`Repeat::Once`] tween has finished.
fn cycle(
    start: Instant,
    now: Instant,
    duration: Duration,
    delay: Duration,
    repeat: Repeat,
) -> (f32, bool) {
    let Some(elapsed) = motion_elapsed(start, now, delay) else {
        return (0.0, false);
    };
    if duration.is_zero() {
        return (1.0, true);
    }
    let cycles = elapsed / duration.as_secs_f32();
    match repeat {
        Repeat::Once => (cycles.min(1.0), cycles >= 1.0),
        Repeat::Loop => (cycles.fract(), false),
        // Each duration is one leg. The ease restarts at 0; the driver swaps the ends on the
        // way back, so left-to-right and right-to-left are two cycles of the same curve.
        Repeat::PingPong => (cycles.fract(), false),
    }
}

fn drive<T: Copy>(
    slot: &mut Option<Tween<T>>,
    now: Instant,
    lerp: impl Fn(T, T, f32) -> T,
) -> Option<T> {
    let mut tween = slot.take()?;
    if tween.start.is_none() {
        tween.start = Some(now);
    }
    let start = tween.start.expect("the clock was just filled");
    let (linear, done) = cycle(start, now, tween.duration, tween.delay, tween.repeat);
    if done {
        return Some(tween.to);
    }
    let eased = tween.ease.apply(linear);
    let returning = tween.repeat == Repeat::PingPong
        && motion_elapsed(start, now, tween.delay)
            .is_some_and(|elapsed| !leg_is_forward(elapsed, tween.duration));
    let value = if returning {
        lerp(tween.to, tween.from, eased)
    } else {
        lerp(tween.from, tween.to, eased)
    };
    *slot = Some(tween);
    Some(value)
}

/// Even legs run toward the destination. Odd legs run back, as a second cycle.
fn leg_is_forward(elapsed: f32, duration: Duration) -> bool {
    if duration.is_zero() {
        return true;
    }
    let turns = (elapsed / duration.as_secs_f32()).floor();
    (turns as u64).is_multiple_of(2)
}

fn lerp_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn lerp_point(from: Point, to: Point, t: f32) -> Point {
    Point::new(lerp_f32(from.x, to.x, t), lerp_f32(from.y, to.y, t))
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    Color {
        r: lerp_f32(from.r, to.r, t).clamp(0.0, 1.0),
        g: lerp_f32(from.g, to.g, t).clamp(0.0, 1.0),
        b: lerp_f32(from.b, to.b, t).clamp(0.0, 1.0),
        a: lerp_f32(from.a, to.a, t).clamp(0.0, 1.0),
    }
}

fn rest_background(node: &super::node::Node) -> Color {
    match node.style.visual.background {
        Background::Solid(color) | Background::Linear { from: color, .. } => color,
    }
}

fn tween<T: Copy>(from: T, to: T, motion: Motion) -> Tween<T> {
    Tween {
        from,
        to,
        start: None,
        duration: motion.duration,
        delay: motion.delay,
        ease: motion.ease,
        repeat: motion.repeat,
        owner: None,
    }
}

impl Ui {
    /// Replaces the tween for this motion's property. `from` is the value on screen and the
    /// duration clock starts on the next sample.
    pub fn animate(&mut self, id: NodeId, motion: Motion) {
        if !self.is_current(id) || self.nodes[id.index()].suppressed {
            return;
        }
        let node = &mut self.nodes[id.index()];
        match motion.kind {
            MotionKind::Translation(to) => {
                node.shake = None;
                node.translation_tween = Some(tween(node.translation, to, motion));
            }
            MotionKind::Opacity(to) => {
                node.opacity_tween = Some(tween(node.opacity, to, motion));
            }
            MotionKind::Background(to) => {
                let from = node.background.unwrap_or_else(|| rest_background(node));
                node.background_tween = Some(tween(from, to, motion));
            }
            MotionKind::Scale(to) => {
                node.scale_tween = Some(tween(node.scale, to, motion));
            }
            MotionKind::Rotation(degrees) => {
                node.rotation_tween = Some(tween(node.rotation, degrees.to_radians(), motion));
            }
            MotionKind::Shake(amplitude) => {
                node.translation_tween = None;
                node.shake = Some(Wave {
                    amplitude,
                    base: node.translation,
                    start: None,
                    duration: motion.duration,
                    delay: motion.delay,
                    repeat: motion.repeat,
                    owner: None,
                });
            }
        }
    }

    pub(super) fn motion_animating(&self) -> bool {
        !self.anims.is_empty()
            || self.nodes.iter().any(|node| {
                node.alive
                    && (node.translation_tween.is_some()
                        || node.opacity_tween.is_some()
                        || node.background_tween.is_some()
                        || node.scale_tween.is_some()
                        || node.rotation_tween.is_some()
                        || node.shake.is_some())
            })
    }

    /// Queues `anim` to start during the next [`Ui::step_motion`] in this update.
    pub fn play(&mut self, anim: Anim) -> AnimHandle {
        let id = self.next_anim_owner();
        self.anims.push(Running {
            id,
            active: Active::from_clip(anim.clip),
        });
        AnimHandle {
            id,
            runtime_index: crate::reactive::register_runtime(&self.queue.runtime),
        }
    }

    /// Drops the group and the tweens it still owns. The value on screen stays put.
    pub fn stop(&mut self, handle: AnimHandle) {
        let Some(index) = self
            .anims
            .iter()
            .position(|running| running.id == handle.id)
        else {
            return;
        };
        let running = self.anims.remove(index);
        self.release_active(&running.active);
    }

    /// Advances groups. A step that just finished installs the next motion, then that motion is
    /// sampled so a chain of zero-length clips does not wait a frame between steps.
    pub(super) fn finish_anims(&mut self, now: Instant) {
        for _ in 0..1024 {
            if self.anims.is_empty() {
                return;
            }
            let mut anims = std::mem::take(&mut self.anims);
            let mut started = false;
            anims.retain_mut(|running| {
                let (done, began) = self.poll_anim(&mut running.active);
                started |= began;
                !done
            });
            self.anims = anims;
            if !started {
                return;
            }
            self.step_motion(now);
        }
    }

    fn poll_anim(&mut self, anim: &mut Active) -> (bool, bool) {
        match anim {
            Active::Play {
                id,
                motion,
                owner,
                started,
            } => {
                if !*started {
                    *owner = self.next_anim_owner();
                    let token = *owner;
                    let kind = motion.kind;
                    self.animate(*id, *motion);
                    self.stamp(*id, kind, token);
                    *started = true;
                    let running = self.owns(*id, kind, token);
                    return (!running, running);
                }
                (!self.owns(*id, motion.kind, *owner), false)
            }
            Active::Together(children) => {
                let mut started_any = false;
                children.retain_mut(|child| {
                    let (done, began) = self.poll_anim(child);
                    started_any |= began;
                    !done
                });
                (children.is_empty(), started_any)
            }
            Active::Sequence { pending, current } => {
                let mut started_any = false;
                loop {
                    if current.is_none() {
                        if pending.is_empty() {
                            return (true, started_any);
                        }
                        let next = pending.remove(0);
                        *current = Some(Box::new(Active::from_clip(next)));
                    }
                    let (done, began) =
                        self.poll_anim(current.as_mut().expect("a step is current"));
                    started_any |= began;
                    if !done {
                        return (false, started_any);
                    }
                    *current = None;
                }
            }
        }
    }

    fn next_anim_owner(&mut self) -> u64 {
        self.next_anim_owner = self.next_anim_owner.wrapping_add(1).max(1);
        self.next_anim_owner
    }

    fn stamp(&mut self, id: NodeId, kind: MotionKind, owner: u64) {
        let Some(node) = self.nodes.get_mut(id.index()) else {
            return;
        };
        if node.generation != id.generation() {
            return;
        }
        match kind {
            MotionKind::Translation(_) => {
                if let Some(tween) = node.translation_tween.as_mut() {
                    tween.owner = Some(owner);
                }
            }
            MotionKind::Opacity(_) => {
                if let Some(tween) = node.opacity_tween.as_mut() {
                    tween.owner = Some(owner);
                }
            }
            MotionKind::Background(_) => {
                if let Some(tween) = node.background_tween.as_mut() {
                    tween.owner = Some(owner);
                }
            }
            MotionKind::Scale(_) => {
                if let Some(tween) = node.scale_tween.as_mut() {
                    tween.owner = Some(owner);
                }
            }
            MotionKind::Rotation(_) => {
                if let Some(tween) = node.rotation_tween.as_mut() {
                    tween.owner = Some(owner);
                }
            }
            MotionKind::Shake(_) => {
                if let Some(wave) = node.shake.as_mut() {
                    wave.owner = Some(owner);
                }
            }
        }
    }

    fn owns(&self, id: NodeId, kind: MotionKind, owner: u64) -> bool {
        let Some(node) = self.nodes.get(id.index()) else {
            return false;
        };
        if !node.alive || node.generation != id.generation() {
            return false;
        }
        let token = Some(owner);
        match kind {
            MotionKind::Translation(_) => node
                .translation_tween
                .as_ref()
                .is_some_and(|tween| tween.owner == token),
            MotionKind::Opacity(_) => node
                .opacity_tween
                .as_ref()
                .is_some_and(|tween| tween.owner == token),
            MotionKind::Background(_) => node
                .background_tween
                .as_ref()
                .is_some_and(|tween| tween.owner == token),
            MotionKind::Scale(_) => node
                .scale_tween
                .as_ref()
                .is_some_and(|tween| tween.owner == token),
            MotionKind::Rotation(_) => node
                .rotation_tween
                .as_ref()
                .is_some_and(|tween| tween.owner == token),
            MotionKind::Shake(_) => node.shake.as_ref().is_some_and(|wave| wave.owner == token),
        }
    }

    fn release_active(&mut self, active: &Active) {
        match active {
            Active::Play {
                id,
                motion,
                owner,
                started: true,
            } => self.clear_owned(*id, motion.kind, *owner),
            Active::Play { started: false, .. } => {}
            Active::Together(children) => {
                for child in children {
                    self.release_active(child);
                }
            }
            Active::Sequence { current, .. } => {
                if let Some(current) = current {
                    self.release_active(current);
                }
            }
        }
    }

    /// Drops the tween this group started. Leaves the property at the value already on screen.
    fn clear_owned(&mut self, id: NodeId, kind: MotionKind, owner: u64) {
        if !self.owns(id, kind, owner) {
            return;
        }
        let node = &mut self.nodes[id.index()];
        match kind {
            MotionKind::Translation(_) => node.translation_tween = None,
            MotionKind::Opacity(_) => node.opacity_tween = None,
            MotionKind::Background(_) => node.background_tween = None,
            MotionKind::Scale(_) => node.scale_tween = None,
            MotionKind::Rotation(_) => node.rotation_tween = None,
            MotionKind::Shake(_) => node.shake = None,
        }
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

    /// Samples running tweens. A finished [`Repeat::Once`] tween drops its clock and leaves the
    /// property on the destination. Loop and ping-pong keep the clock.
    pub(super) fn step_motion(&mut self, now: Instant) {
        let mut moved = false;
        let mut visual = Vec::new();
        for index in 0..self.nodes.len() {
            if !self.nodes[index].alive {
                continue;
            }
            let previous_translation = self.nodes[index].translation;
            if let Some(value) = drive(&mut self.nodes[index].translation_tween, now, lerp_point) {
                self.nodes[index].translation = value;
            }
            if let Some(mut wave) = self.nodes[index].shake {
                if wave.start.is_none() {
                    wave.start = Some(now);
                }
                let start = wave.start.expect("the clock was just filled");
                let (linear, done) = cycle(start, now, wave.duration, wave.delay, wave.repeat);
                if done {
                    self.nodes[index].shake = None;
                    self.nodes[index].translation = wave.base;
                } else {
                    let offset = (linear * PI * 2.0).sin() * wave.amplitude;
                    self.nodes[index].translation = Point::new(wave.base.x + offset, wave.base.y);
                    self.nodes[index].shake = Some(wave);
                }
            }
            if self.nodes[index].translation != previous_translation {
                moved = true;
            }
            let previous_opacity = self.nodes[index].opacity;
            if let Some(value) = drive(&mut self.nodes[index].opacity_tween, now, lerp_f32) {
                self.nodes[index].opacity = value;
            }
            let previous_background = self.nodes[index].background;
            if let Some(value) = drive(&mut self.nodes[index].background_tween, now, lerp_color) {
                self.nodes[index].background = Some(value);
            }
            let previous_scale = self.nodes[index].scale;
            if let Some(value) = drive(&mut self.nodes[index].scale_tween, now, lerp_f32) {
                self.nodes[index].scale = value.max(0.0);
            }
            let previous_rotation = self.nodes[index].rotation;
            if let Some(value) = drive(&mut self.nodes[index].rotation_tween, now, lerp_f32) {
                self.nodes[index].rotation = value;
            }
            let node = &self.nodes[index];
            if node.opacity != previous_opacity
                || node.background != previous_background
                || node.scale != previous_scale
                || node.rotation != previous_rotation
            {
                visual.push(index);
            }
        }
        if moved {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
        for index in visual {
            let id = NodeId::new(index as u32, self.nodes[index].generation);
            self.damage_visual_tree(id);
        }
    }

    pub(super) fn cancel_motion(&mut self, id: NodeId) {
        if !self.is_current(id) {
            return;
        }
        self.anims.retain(|running| !running.active.mentions(id));
        let node = &mut self.nodes[id.index()];
        let moved = node.translation != Point::default()
            || node.translation_tween.is_some()
            || node.shake.is_some();
        let painted = node.opacity != 1.0
            || node.opacity_tween.is_some()
            || node.background.is_some()
            || node.background_tween.is_some()
            || (node.scale - 1.0).abs() > 1.0e-4
            || node.scale_tween.is_some()
            || node.rotation.abs() > 1.0e-4
            || node.rotation_tween.is_some();
        node.translation_tween = None;
        node.opacity_tween = None;
        node.background_tween = None;
        node.scale_tween = None;
        node.rotation_tween = None;
        node.shake = None;
        node.translation = Point::default();
        node.opacity = 1.0;
        node.background = None;
        node.scale = 1.0;
        node.rotation = 0.0;
        if moved {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
        if painted {
            node.paint_dirty = true;
            self.paint_dirty = true;
            self.display_list_stale = true;
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
