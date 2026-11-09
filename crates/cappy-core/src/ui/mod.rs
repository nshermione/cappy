//! The UI runtime: owns the retained node tree and turns input and signal changes into
//! damaged regions plus a display list. It knows nothing about windows or GPUs.

mod bindings;
pub(crate) mod flow;
mod input;
mod motion;
mod node;
mod passes;
mod scroll;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use cappy_layout::{ApproximateTextMeasurer, LayoutTree, Point, Rect, Size, TextMeasurer};
use cappy_style::{Style, Theme};

use crate::element::IntoElement;
use crate::paint::{Primitive, RenderFrame};
use crate::reactive::{BindingQueue, Signal};
use crate::{Context, CursorIcon, Damage, DamageTracker, ThemeHandle};
use bindings::{Binding, BindingKind};
pub use motion::{Ease, Motion, MotionCtx};
pub use node::NodeId;
use node::{Node, NodeContent};

/// How many slots cappy still holds. [`Ui::resources`] fills one of these.
///
/// Length is the slots in use (including disposed slots not yet released). Capacity is the
/// memory reserved for them. A value that returns to an earlier snapshot released what it created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceCensus {
    pub nodes: usize,
    pub node_capacity: usize,
    pub bindings: usize,
    pub binding_capacity: usize,
    pub flows: usize,
    pub flow_capacity: usize,
    pub layout_nodes: usize,
    pub layout_capacity: usize,
    /// Taffy slots reserved, including ones `remove` has not given back yet.
    pub layout_retained: usize,
    pub binding_sources: usize,
    pub binding_source_capacity: usize,
    pub subscriptions: usize,
    pub display_list: usize,
    pub display_list_capacity: usize,
    pub signal_slots: usize,
    pub signal_capacity: usize,
    pub live_signals: usize,
    pub signal_subscribers: usize,
    pub owners: usize,
    pub owner_capacity: usize,
    pub live_owners: usize,
    pub text_measurements: usize,
    pub listeners: usize,
    pub listener_capacity: usize,
    pub live_listeners: usize,
    pub singletons: usize,
}

/// Size and scale of the surface the UI is drawn on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameInput {
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f32,
}

impl FrameInput {
    pub fn new(physical_width: u32, physical_height: u32, scale_factor: f32) -> Self {
        Self {
            physical_width,
            physical_height,
            scale_factor,
        }
    }

    pub fn logical_size(&self) -> Size {
        Size::new(
            self.physical_width as f32 / self.scale_factor,
            self.physical_height as f32 / self.scale_factor,
        )
    }
}

/// Result of [`Ui::update`].
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateOutcome {
    /// Nothing changed on screen; the host does not need to redraw.
    Idle,
    /// Some regions changed; the host should redraw (see [`Ui::prepare_frame`]).
    Redraw { damage: Damage },
}

impl UpdateOutcome {
    pub fn needs_redraw(&self) -> bool {
        matches!(self, Self::Redraw { .. })
    }
}

pub struct Ui {
    nodes: Vec<Node>,
    /// Indexes of disposed nodes, reused by the next mount.
    free_nodes: Vec<u32>,
    root: NodeId,
    bindings: Vec<Binding>,
    /// Indexes of disposed bindings, reused by the next allocation.
    free_bindings: Vec<u32>,
    flows: Vec<flow::FlowState>,
    queue: Rc<BindingQueue>,
    theme_signal: Signal<ThemeHandle>,
    theme: ThemeHandle,
    layout: LayoutTree<NodeId>,
    measurer: Box<dyn TextMeasurer>,
    viewport: Option<FrameInput>,
    damage: DamageTracker,
    display_list: Arc<Vec<Primitive>>,
    display_list_stale: bool,
    style_dirty: bool,
    layout_dirty: bool,
    /// Child positions must be rewritten from the last taffy result. Scroll changes this
    /// without running taffy again.
    positions_dirty: bool,
    paint_dirty: bool,
    /// Instant of the previous [`Ui::update`], used to ease scroll offsets.
    updated_at: Option<Instant>,
    pointer: Option<Point>,
    hovered: Vec<NodeId>,
    pressed: Option<NodeId>,
    /// Thumb or track drag. The offset follows the pointer until the button is released.
    scrollbar_drag: Option<input::ScrollbarDrag>,
}

impl Ui {
    /// Mounts `root` on the context theme.
    ///
    /// When the context has no themes yet, installs `theme` as the only one. When themes are
    /// already set, `theme` is ignored and the active theme stays. Switch with
    /// [`Context::apply_theme`](crate::Context::apply_theme) or [`Ui::set_theme`].
    ///
    /// The theme signal belongs to the context. Dropping this `Ui` does not dispose it.
    pub fn new(context: &Context, root: impl IntoElement, theme: Theme) -> Self {
        if !context.runtime.has_themes() {
            context.set_themes([theme]);
        }
        let theme_signal = context.current_theme().as_signal();
        let initial_theme = theme_signal.get_untracked();
        let mut ui = Self {
            nodes: Vec::new(),
            free_nodes: Vec::new(),
            root: NodeId::from_index(0),
            bindings: Vec::new(),
            free_bindings: Vec::new(),
            flows: Vec::new(),
            queue: Rc::new(BindingQueue::new(Rc::clone(&context.runtime))),
            theme_signal,
            theme: initial_theme,
            layout: LayoutTree::new(),
            measurer: Box::new(ApproximateTextMeasurer::default()),
            viewport: None,
            damage: DamageTracker::default(),
            display_list: Arc::new(Vec::new()),
            display_list_stale: true,
            style_dirty: true,
            layout_dirty: true,
            positions_dirty: true,
            paint_dirty: true,
            updated_at: None,
            pointer: None,
            hovered: Vec::new(),
            pressed: None,
            scrollbar_drag: None,
        };
        ui.root = ui.mount(root.into_element(), None);
        ui.add_binding(ui.root, BindingKind::Theme);
        ui.queue.runtime.attach_ui();
        ui
    }

    /// Replaces the text measurer (normally the renderer's font system) and re-measures text.
    pub fn set_text_measurer(&mut self, measurer: impl TextMeasurer + 'static) {
        self.measurer = Box::new(measurer);
        for index in 0..self.nodes.len() {
            if self.nodes[index].alive {
                let id = NodeId::new(index as u32, self.nodes[index].generation);
                self.sync_text_leaf(id);
            }
        }
    }

    /// Called whenever a signal change makes the UI need an update, so a host sleeping in an
    /// event loop can wake up. Fires at most once until the next [`Ui::update`].
    pub fn set_waker(&mut self, waker: impl Fn() + 'static) {
        self.queue.set_waker(Some(Rc::new(waker)));
    }

    pub fn theme(&self) -> &ThemeHandle {
        &self.theme
    }

    /// The context theme this UI follows. The signal stays alive after this `Ui` drops.
    pub fn theme_signal(&self) -> Signal<ThemeHandle> {
        self.theme_signal
    }

    /// True when `context` is the runtime this UI mounts signals into.
    pub fn shares_context(&self, context: &Context) -> bool {
        Rc::ptr_eq(&self.queue.runtime, &context.runtime)
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme_signal.set(ThemeHandle::from(theme));
    }

    /// True when [`Ui::update`] has work to do.
    pub fn needs_update(&self) -> bool {
        self.viewport.is_none()
            || !self.queue.is_empty()
            || self.style_dirty
            || self.layout_dirty
            || self.positions_dirty
            || self.paint_dirty
            || self.scroll_animating()
            || self.motion_animating()
            || self.queue.runtime.has_motions()
    }

    /// When the host should draw the next animation frame. `None` once scroll and tweens rest.
    pub fn frame_deadline(&self, now: Instant) -> Option<Instant> {
        (self.scroll_animating() || self.motion_animating()).then_some(now + scroll::SCROLL_FRAME)
    }

    fn scroll_animating(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| node.alive && scroll::scrolling(node.scroll, node.scroll_target))
    }

    /// Applies pending signal changes, restyles, relayouts and collects damage.
    pub fn update(&mut self, input: FrameInput) -> UpdateOutcome {
        self.update_at(input, Instant::now())
    }

    /// [`Ui::update`] at `now`, so tests can step the scroll ease without waiting.
    pub(crate) fn update_at(&mut self, input: FrameInput, now: Instant) -> UpdateOutcome {
        let dt = scroll::scroll_dt(self.updated_at, now);
        self.updated_at = Some(now);
        self.apply_viewport(input);
        self.process_bindings();
        self.resolve_styles();
        self.run_layout();
        // Hover changes caused by layout can restyle nodes again.
        self.resolve_styles();
        self.run_layout();
        self.drain_motions();
        self.step_scroll(dt);
        self.step_motion(now);
        self.run_layout();
        self.collect_paint_damage();
        self.rebuild_display_list();
        self.reclaim_unused();
        if self.damage.is_empty() && !self.scroll_animating() && !self.motion_animating() {
            UpdateOutcome::Idle
        } else {
            UpdateOutcome::Redraw {
                damage: self.damage.preview(input.logical_size()),
            }
        }
    }

    /// Hands the current display list and the accumulated damage to a renderer, then resets the
    /// damage. Returns `None` before the first [`Ui::update`].
    pub fn prepare_frame(&mut self) -> Option<RenderFrame> {
        let viewport = self.viewport?;
        self.rebuild_display_list();
        Some(RenderFrame {
            physical_width: viewport.physical_width,
            physical_height: viewport.physical_height,
            scale_factor: viewport.scale_factor,
            primitives: self.display_list.clone(),
            damage: self.damage.take(viewport.logical_size()),
        })
    }

    pub fn cursor(&self) -> CursorIcon {
        let over_clickable = self
            .hovered
            .iter()
            .any(|id| self.is_current(*id) && self.nodes[id.index()].on_click.is_some());
        if over_clickable {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn display_list(&self) -> &[Primitive] {
        &self.display_list
    }

    fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes
            .get(id.index())
            .filter(|node| node.generation == id.generation())
    }

    pub fn find_by_key(&self, key: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|node| node.alive && node.key.as_deref() == Some(key))
            .map(|index| NodeId::new(index as u32, self.nodes[index].generation))
    }

    pub fn node_rect(&self, id: NodeId) -> Option<Rect> {
        self.node(id).map(|node| node.rect)
    }

    pub fn node_style(&self, id: NodeId) -> Option<&Style> {
        self.node(id).map(|node| &node.style)
    }

    pub fn node_text(&self, id: NodeId) -> Option<&str> {
        self.node(id).and_then(Node::text)
    }

    pub fn node_children(&self, id: NodeId) -> &[NodeId] {
        self.node(id).map_or(&[], |node| &node.children)
    }

    pub fn node_parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).and_then(|node| node.parent)
    }

    fn logical_viewport(&self) -> Size {
        self.viewport
            .map(|viewport| viewport.logical_size())
            .unwrap_or_default()
    }

    fn apply_viewport(&mut self, input: FrameInput) {
        let previous = self.viewport.replace(input);
        if previous == Some(input) {
            return;
        }
        self.damage.add_full();
        if previous.map(|viewport| viewport.logical_size()) != Some(input.logical_size()) {
            self.layout_dirty = true;
        }
    }

    #[cfg(test)]
    pub(super) fn node_slots(&self) -> usize {
        self.nodes.len()
    }

    #[cfg(test)]
    pub(super) fn binding_slots(&self) -> usize {
        self.bindings.len()
    }

    /// Releases arena slots that a dispose left at the end, so a shrink returns that memory.
    fn reclaim_unused(&mut self) {
        while self.nodes.last().is_some_and(|node| !node.alive) {
            self.nodes.pop();
        }
        let node_len = self.nodes.len() as u32;
        self.free_nodes.retain(|index| *index < node_len);
        self.nodes.shrink_to_fit();
        self.free_nodes.shrink_to_fit();

        while self.bindings.last().is_some_and(|binding| binding.disposed) {
            self.bindings.pop();
        }
        let binding_len = self.bindings.len() as u32;
        self.free_bindings.retain(|index| *index < binding_len);
        self.bindings.shrink_to_fit();
        self.free_bindings.shrink_to_fit();
        self.queue.shrink_sources(self.bindings.len());

        self.reclaim_disposed_flows();
        self.layout.compact();
        self.layout.shrink_to_fit();
        let hovered = std::mem::take(&mut self.hovered);
        self.hovered = hovered
            .into_iter()
            .filter(|id| self.is_current(*id))
            .collect();
        self.hovered.shrink_to_fit();
        if self.pressed.is_some_and(|id| !self.is_current(id)) {
            self.pressed = None;
        }
        if self
            .scrollbar_drag
            .as_ref()
            .is_some_and(|drag| !self.is_current(drag.id))
        {
            self.scrollbar_drag = None;
        }
        crate::reactive::reclaim_unused_slots(&self.queue.runtime);
    }

    /// Counts the arenas this UI still holds, including signals and owners in its context.
    ///
    /// Take a snapshot, run the operation, and compare. Another `Ui` on another context has its
    /// own counts.
    pub fn resources(&self) -> ResourceCensus {
        let runtime = crate::reactive::runtime_census(&self.queue.runtime);
        ResourceCensus {
            nodes: self.nodes.len(),
            node_capacity: self.nodes.capacity(),
            bindings: self.bindings.len(),
            binding_capacity: self.bindings.capacity(),
            flows: self.flows.len(),
            flow_capacity: self.flows.capacity(),
            layout_nodes: self.layout.node_count(),
            layout_capacity: self.layout.node_capacity(),
            layout_retained: self.layout.retained_slots(),
            binding_sources: self.queue.source_slots(),
            binding_source_capacity: self.queue.source_capacity(),
            subscriptions: self.queue.subscription_count(),
            display_list: self.display_list.len(),
            display_list_capacity: self.display_list.capacity(),
            signal_slots: runtime.signal_slots,
            signal_capacity: runtime.signal_capacity,
            live_signals: runtime.live_signals,
            signal_subscribers: runtime.signal_subscribers,
            owners: runtime.owners,
            owner_capacity: runtime.owner_capacity,
            live_owners: runtime.live_owners,
            text_measurements: self.measurer.cached_measurements(),
            listeners: runtime.listeners,
            listener_capacity: runtime.listener_capacity,
            live_listeners: runtime.live_listeners,
            singletons: runtime.singletons,
        }
    }
}

impl Context {
    /// Queues `apply` to run on the next [`Ui::update`](Ui::update), after layout.
    ///
    /// The closure does not run at the call. A context has one [`Ui`]; that `Ui` drains the queue.
    /// A call made before the `Ui` exists waits for its first update. The host waker set on the
    /// `Ui` fires when the queue goes from empty to occupied.
    pub fn animate(&self, apply: impl FnOnce(&mut MotionCtx) + 'static) {
        self.runtime.push_motion(Box::new(move |ui| {
            apply(&mut MotionCtx::new(ui));
        }));
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        self.dispose_tree();
        self.queue.runtime.detach_ui();
    }
}

#[cfg(test)]
mod tests;
