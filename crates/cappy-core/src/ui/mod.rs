//! The UI runtime: owns the retained node tree and turns input and signal changes into
//! damaged regions plus a display list. It knows nothing about windows or GPUs.

mod bindings;
mod input;
mod node;
mod passes;

use std::rc::Rc;
use std::sync::Arc;

use cappy_layout::{ApproximateTextMeasurer, LayoutTree, Point, Rect, Size, TextMeasurer};
use cappy_style::{Style, TextStyle, Theme};

use crate::element::{Element, ElementContent};
use crate::paint::{Primitive, RenderFrame};
use crate::reactive::{BindingQueue, Signal};
use crate::{CursorIcon, Damage, DamageTracker};
use bindings::{Binding, BindingKind};
pub use node::NodeId;
use node::{Node, NodeContent};

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
    root: NodeId,
    bindings: Vec<Binding>,
    queue: Rc<BindingQueue>,
    theme_signal: Signal<Arc<Theme>>,
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
    pub fn new(root: impl Into<Element>, theme: Theme) -> Self {
        Self::with_theme_signal(root, Signal::new(Arc::new(theme)))
    }

    /// Mounts `root` and follows `theme`: setting the signal restyles the whole UI.
    pub fn with_theme_signal(root: impl Into<Element>, theme: Signal<Arc<Theme>>) -> Self {
        let initial_theme = theme.get_untracked();
        let mut ui = Self {
            nodes: Vec::new(),
            root: NodeId(0),
            bindings: Vec::new(),
            queue: Rc::new(BindingQueue::default()),
            theme_signal: theme,
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
            self.sync_text_leaf(NodeId(index as u32));
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
            .any(|id| self.nodes[id.index()].on_click.is_some());
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

    pub fn find_by_key(&self, key: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|node| node.key.as_deref() == Some(key))
            .map(|index| NodeId(index as u32))
    }

    pub fn node_rect(&self, id: NodeId) -> Option<Rect> {
        self.nodes.get(id.index()).map(|node| node.rect)
    }

    pub fn node_style(&self, id: NodeId) -> Option<&Style> {
        self.nodes.get(id.index()).map(|node| &node.style)
    }

    pub fn node_text(&self, id: NodeId) -> Option<&str> {
        self.nodes.get(id.index()).and_then(Node::text)
    }

    pub fn node_children(&self, id: NodeId) -> &[NodeId] {
        self.nodes
            .get(id.index())
            .map_or(&[], |node| &node.children)
    }

    pub fn node_parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.index()).and_then(|node| node.parent)
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

    fn mount(&mut self, element: Element, parent: Option<NodeId>) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("too many nodes"));
        let Element {
            content,
            key,
            classes,
            class_binding,
            style,
            style_binding,
            on_click,
            on_hover,
            children,
        } = element;
        let (content, text_binding) = match content {
            ElementContent::Container => (NodeContent::Container, None),
            ElementContent::StaticText(text) => (NodeContent::Text(text), None),
            ElementContent::DynamicText(binding) => (NodeContent::Text("".into()), Some(binding)),
        };
        let initial_style = Style::inheriting(TextStyle::from_tokens(self.theme.tokens()));
        self.nodes.push(Node {
            parent,
            children: Vec::new(),
            key,
            content,
            classes,
            dynamic_classes: Vec::new(),
            inline_style: style,
            dynamic_style: Default::default(),
            style: initial_style,
            interaction: Default::default(),
            on_click,
            on_hover,
            rect: Rect::ZERO,
            style_dirty: true,
            paint_dirty: true,
        });
        let layout_style = self.layout_style_of(id);
        self.layout
            .insert(id, &layout_style)
            .expect("inserting a fresh layout node cannot fail");
        self.sync_text_leaf(id);
        if let Some(binding) = text_binding {
            self.add_binding(id, BindingKind::Text(binding));
        }
        if let Some(binding) = class_binding {
            self.add_binding(id, BindingKind::Classes(binding));
        }
        if let Some(binding) = style_binding {
            self.add_binding(id, BindingKind::Style(binding));
        }
        let child_ids: Vec<NodeId> = children
            .into_iter()
            .map(|child| self.mount(child, Some(id)))
            .collect();
        self.layout
            .set_children(id, &child_ids)
            .expect("children were just inserted into the layout tree");
        self.nodes[id.index()].children = child_ids;
        id
    }
}

#[cfg(test)]
mod tests;
