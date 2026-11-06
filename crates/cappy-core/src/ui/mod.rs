//! The UI runtime: owns the retained node tree and turns input and signal changes into
//! damaged regions plus a display list. It knows nothing about windows or GPUs.

mod bindings;
pub(crate) mod flow;
mod input;
mod node;
mod passes;

use std::rc::Rc;
use std::sync::Arc;

use cappy_layout::{ApproximateTextMeasurer, LayoutTree, Point, Rect, Size, TextMeasurer};
use cappy_style::{Style, Theme};

use crate::element::Element;
use crate::paint::{Primitive, RenderFrame};
use crate::reactive::{BindingQueue, Signal};
use crate::{CursorIcon, Damage, DamageTracker};
use bindings::{Binding, BindingKind};
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
    theme_signal: Signal<Arc<Theme>>,
    /// [`Ui::new`] allocated `theme_signal` and disposes it on drop.
    owns_theme: bool,
    theme: Arc<Theme>,
    layout: LayoutTree<NodeId>,
    measurer: Box<dyn TextMeasurer>,
    viewport: Option<FrameInput>,
    damage: DamageTracker,
    display_list: Arc<Vec<Primitive>>,
    display_list_stale: bool,
    style_dirty: bool,
    layout_dirty: bool,
    paint_dirty: bool,
    pointer: Option<Point>,
    hovered: Vec<NodeId>,
    pressed: Option<NodeId>,
}

impl Ui {
    /// Mounts `root` with a fixed theme. Use [`Ui::set_theme`] or [`Ui::theme_signal`] to switch.
    ///
    /// The theme signal created here is disposed when this `Ui` drops.
    pub fn new(root: impl Into<Element>, theme: Theme) -> Self {
        Self::with_owned_theme(root, Signal::new(Arc::new(theme)), true)
    }

    /// Mounts `root` and follows `theme`: setting the signal restyles the whole UI.
    ///
    /// `theme` stays alive after this `Ui` drops. The caller owns that signal.
    pub fn with_theme_signal(root: impl Into<Element>, theme: Signal<Arc<Theme>>) -> Self {
        Self::with_owned_theme(root, theme, false)
    }

    fn with_owned_theme(
        root: impl Into<Element>,
        theme: Signal<Arc<Theme>>,
        owns_theme: bool,
    ) -> Self {
        let initial_theme = theme.get_untracked();
        let mut ui = Self {
            nodes: Vec::new(),
            free_nodes: Vec::new(),
            root: NodeId::from_index(0),
            bindings: Vec::new(),
            free_bindings: Vec::new(),
            flows: Vec::new(),
            queue: Rc::new(BindingQueue::default()),
            theme_signal: theme,
            owns_theme,
            theme: initial_theme,
            layout: LayoutTree::new(),
            measurer: Box::new(ApproximateTextMeasurer::default()),
            viewport: None,
            damage: DamageTracker::default(),
            display_list: Arc::new(Vec::new()),
            display_list_stale: true,
            style_dirty: true,
            layout_dirty: true,
            paint_dirty: true,
            pointer: None,
            hovered: Vec::new(),
            pressed: None,
        };
        ui.root = ui.mount(root.into(), None);
        ui.add_binding(ui.root, BindingKind::Theme);
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

    pub fn theme(&self) -> &Arc<Theme> {
        &self.theme
    }

    /// The theme this UI follows.
    ///
    /// When the UI was built with [`Ui::new`], the signal is disposed as the `Ui` drops, and any
    /// copy kept outside panics on the next read. A signal passed to [`Ui::with_theme_signal`]
    /// is not disposed.
    pub fn theme_signal(&self) -> Signal<Arc<Theme>> {
        self.theme_signal
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme_signal.set(Arc::new(theme));
    }

    /// True when [`Ui::update`] has work to do.
    pub fn needs_update(&self) -> bool {
        self.viewport.is_none()
            || !self.queue.is_empty()
            || self.style_dirty
            || self.layout_dirty
            || self.paint_dirty
    }

    /// Applies pending signal changes, restyles, relayouts and collects damage.
    pub fn update(&mut self, input: FrameInput) -> UpdateOutcome {
        self.apply_viewport(input);
        self.process_bindings();
        self.resolve_styles();
        self.run_layout();
        // Hover changes caused by layout can restyle nodes again.
        self.resolve_styles();
        self.run_layout();
        self.collect_paint_damage();
        self.rebuild_display_list();
        self.reclaim_unused();
        if self.damage.is_empty() {
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
        crate::reactive::reclaim_unused_slots();
    }

    /// Counts every arena this UI and the thread-local reactive runtime still hold.
    ///
    /// Take a snapshot, run the operation, and compare. The reactive fields cover the whole
    /// thread, so compare two snapshots from the same thread.
    pub fn resources(&self) -> ResourceCensus {
        let runtime = crate::reactive::runtime_census();
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
        }
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        self.dispose_tree();
        if self.owns_theme {
            self.theme_signal.dispose();
        }
    }
}

#[cfg(test)]
mod tests;
