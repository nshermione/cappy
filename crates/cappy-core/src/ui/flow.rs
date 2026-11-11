//! `show`, `if` / `else`, `for` and the list window. Each block is one binding on the parent.
//! `show` hides. `if`, `for` and the window dispose what they drop, walking every nested block
//! the same way.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use cappy_style::{Dimension, FlexDirection, Style, StyleRefinement, TextStyle};

use super::Ui;
use super::bindings::BindingKind;
use super::edit::TextEdit;
use super::node::{ChildPart, ComponentId, InputField, Node, NodeContent, NodeId};
use crate::element::{Element, ElementContent, InputValue, div};
use crate::lifecycle::{Lifecycle, run_hooks};
use crate::reactive::{BindingId, Observer, Signal, dispose_owner, enter_owner, track};

/// What an [`Element`](Element) stores until mount moves it into a [`FlowState`].
pub(crate) enum FlowSpec {
    Show {
        when: Box<dyn Fn() -> bool>,
        view: Box<dyn Fn() -> Element>,
    },
    Branch {
        when: Box<dyn Fn() -> bool>,
        then_branch: Box<dyn Fn() -> Element>,
        else_branch: Box<dyn Fn() -> Element>,
    },
    Repeat(Box<dyn RepeatRuntime>),
    Window(WindowRuntime),
}

pub(crate) fn show_spec(
    when: impl Fn() -> bool + 'static,
    view: impl Fn() -> Element + 'static,
) -> FlowSpec {
    FlowSpec::Show {
        when: Box::new(when),
        view: Box::new(view),
    }
}

pub(crate) fn branch_spec(
    when: impl Fn() -> bool + 'static,
    then_branch: impl Fn() -> Element + 'static,
    else_branch: impl Fn() -> Element + 'static,
) -> FlowSpec {
    FlowSpec::Branch {
        when: Box::new(when),
        then_branch: Box::new(then_branch),
        else_branch: Box::new(else_branch),
    }
}

pub(crate) fn repeat_spec<T, K>(
    read: impl Fn(&mut dyn FnMut(&T)) + 'static,
    key_of: impl for<'a> Fn(&'a T) -> &'a K + 'static,
    view: impl Fn(Signal<T>) -> Element + 'static,
) -> FlowSpec
where
    T: Clone + PartialEq + 'static,
    K: ToOwned + Eq + Hash + ?Sized + 'static,
    K::Owned: Clone + Eq + Hash + Borrow<K> + 'static,
{
    FlowSpec::Repeat(Box::new(ListRuntime {
        read: Box::new(read),
        key_of: Box::new(key_of),
        view: Box::new(view),
        rows: Vec::new(),
        index: HashMap::new(),
    }))
}

pub(crate) fn window_spec(
    scroller: String,
    count: impl Fn() -> usize + 'static,
    extent: f32,
    buffer: usize,
    view: impl Fn(usize) -> Element + 'static,
) -> FlowSpec {
    FlowSpec::Window(WindowRuntime {
        scroller,
        count: Box::new(count),
        extent,
        buffer,
        view: Box::new(view),
        rows: Vec::new(),
        before: None,
        after: None,
        placed: false,
        start: 0,
        end: 0,
        len: 0,
    })
}

pub(crate) trait RepeatRuntime {
    fn reconcile(&mut self, ui: &mut Ui, flow: FlowId);
    fn dispose(&mut self, ui: &mut Ui);
}

#[derive(Clone, Copy)]
pub(crate) struct FlowId(u32);

impl FlowId {
    fn index(self) -> usize {
        self.0 as usize
    }

    fn part(self) -> ChildPart {
        ChildPart::Flow(self.0)
    }
}

pub(super) struct FlowState {
    parent: NodeId,
    binding: BindingId,
    runtime: FlowRuntime,
    parts: Vec<ChildPart>,
    owner: Option<crate::reactive::OwnerId>,
    ancestor_suppressed: bool,
    /// `show` is currently hidden. Other blocks leave this false.
    hidden: bool,
    ready: bool,
    disposed: bool,
}

enum FlowRuntime {
    Show(ShowRuntime),
    Branch(BranchRuntime),
    Repeat(Box<dyn RepeatRuntime>),
    Window(WindowRuntime),
    Busy,
}

struct ShowRuntime {
    when: Box<dyn Fn() -> bool>,
    view: Box<dyn Fn() -> Element>,
}

struct BranchRuntime {
    when: Box<dyn Fn() -> bool>,
    then_branch: Box<dyn Fn() -> Element>,
    else_branch: Box<dyn Fn() -> Element>,
    current: Option<bool>,
}

type ListRead<T> = Box<dyn Fn(&mut dyn FnMut(&T))>;

struct ListRuntime<T, K>
where
    T: 'static,
    K: ToOwned + ?Sized,
{
    read: ListRead<T>,
    key_of: Box<dyn for<'a> Fn(&'a T) -> &'a K>,
    view: Box<dyn Fn(Signal<T>) -> Element>,
    rows: Vec<ListRow<T, K::Owned>>,
    index: HashMap<K::Owned, usize>,
}

struct ListRow<T: 'static, K> {
    key: K,
    value: Signal<T>,
    parts: Vec<ChildPart>,
    owner: crate::reactive::OwnerId,
}

impl FlowSpec {
    fn into_runtime(self) -> FlowRuntime {
        match self {
            Self::Show { when, view } => FlowRuntime::Show(ShowRuntime { when, view }),
            Self::Branch {
                when,
                then_branch,
                else_branch,
            } => FlowRuntime::Branch(BranchRuntime {
                when,
                then_branch,
                else_branch,
                current: None,
            }),
            Self::Repeat(runtime) => FlowRuntime::Repeat(runtime),
            Self::Window(runtime) => FlowRuntime::Window(runtime),
        }
    }
}

/// A component that registered hooks. Not a layout node: [`ChildPart::Component`] splices
/// [`ComponentRecord::parts`] into the parent.
pub(super) struct ComponentRecord {
    pub(super) generation: u32,
    hooks: Lifecycle,
    parts: Vec<ChildPart>,
    pub(super) disposed: bool,
}

impl Ui {
    pub(super) fn mount(&mut self, element: Element, parent: Option<NodeId>) -> NodeId {
        let (hooks, element) = self.open_component(element);
        let id = if element.lifecycle.is_some() {
            self.mount(element, parent)
        } else {
            self.mount_root_element(element, parent)
        };
        if let Some(hooks) = hooks {
            let part = self
                .root_component
                .take()
                .map(ComponentId::part)
                .unwrap_or(ChildPart::Node(id));
            self.root_component = Some(self.close_component(hooks, vec![part]));
        }
        id
    }

    fn mount_root_element(&mut self, element: Element, parent: Option<NodeId>) -> NodeId {
        if parent.is_none()
            && matches!(
                element.content,
                ElementContent::Fragment(_) | ElementContent::Flow(_)
            )
        {
            return self.mount_node(div().child(element), None, false);
        }
        self.mount_node(element, parent, false)
    }

    /// Runs `before_mount` and leaves a nested lifecycle on `element` when this component returned
    /// that child directly. `None` when the element has no hooks.
    fn open_component(&mut self, mut element: Element) -> (Option<Lifecycle>, Element) {
        let Some(mut hooks) = element.lifecycle.take() else {
            return (None, element);
        };
        if let Some(inner) = hooks.inner.take() {
            element.lifecycle = Some(*inner);
        }
        self.fire_hooks(&mut hooks.before_mount);
        (Some(hooks), element)
    }

    fn close_component(&mut self, mut hooks: Lifecycle, parts: Vec<ChildPart>) -> ComponentId {
        self.fire_hooks(&mut hooks.mounted);
        self.insert_component(hooks, parts)
    }

    fn fire_hooks(&self, hooks: &mut [crate::lifecycle::Hook]) {
        run_hooks(&self.queue.runtime, hooks);
    }

    fn insert_component(&mut self, hooks: Lifecycle, parts: Vec<ChildPart>) -> ComponentId {
        let ticks = !hooks.updated.is_empty();
        let id = if let Some(index) = self.free_components.pop() {
            let generation = self.components[index as usize].generation.wrapping_add(1);
            self.components[index as usize] = ComponentRecord {
                generation,
                hooks,
                parts,
                disposed: false,
            };
            ComponentId::new(index, generation)
        } else {
            let index = u32::try_from(self.components.len()).expect("too many components");
            self.components.push(ComponentRecord {
                generation: 0,
                hooks,
                parts,
                disposed: false,
            });
            ComponentId::new(index, 0)
        };
        if ticks {
            self.ticking.push(id);
        }
        id
    }

    fn component_record(&self, id: ComponentId) -> Option<&ComponentRecord> {
        let record = self.components.get(id.index())?;
        if record.disposed || record.generation != id.generation() {
            None
        } else {
            Some(record)
        }
    }

    pub(super) fn tick_components(&mut self) {
        let ticking = self.ticking.clone();
        for id in ticking {
            let mut updated = {
                let Some(record) = self.components.get_mut(id.index()) else {
                    continue;
                };
                if record.disposed || record.generation != id.generation() {
                    continue;
                }
                std::mem::take(&mut record.hooks.updated)
            };
            self.fire_hooks(&mut updated);
            if let Some(record) = self.components.get_mut(id.index())
                && !record.disposed
                && record.generation == id.generation()
            {
                record.hooks.updated = updated;
            }
        }
    }

    pub(super) fn reclaim_disposed_components(&mut self) {
        while self.components.last().is_some_and(|record| record.disposed) {
            self.components.pop();
        }
        let len = self.components.len() as u32;
        self.free_components.retain(|index| *index < len);
        self.components.shrink_to_fit();
        self.free_components.shrink_to_fit();
        let ticking: Vec<ComponentId> = self.ticking.clone();
        self.ticking = ticking
            .into_iter()
            .filter(|id| self.component_record(*id).is_some())
            .collect();
        self.ticking.shrink_to_fit();
    }

    fn insert_node(&mut self, mut node: Node) -> NodeId {
        if let Some(index) = self.free_nodes.pop() {
            node.generation = self.nodes[index as usize].generation.wrapping_add(1);
            let id = NodeId::new(index, node.generation);
            self.nodes[index as usize] = node;
            id
        } else {
            let index = u32::try_from(self.nodes.len()).expect("too many nodes");
            node.generation = 0;
            self.nodes.push(node);
            NodeId::new(index, 0)
        }
    }

    fn mount_node(&mut self, element: Element, parent: Option<NodeId>, suppressed: bool) -> NodeId {
        let Element {
            content,
            key,
            classes,
            class_binding,
            style,
            style_binding,
            stylesheet,
            on_click,
            on_hover,
            focusable,
            focus_scope,
            on_key,
            on_text,
            on_ime,
            on_focus,
            on_pointer,
            on_dismiss,
            disabled,
            disabled_when: element_disabled_when,
            caret,
            caret_binding,
            selection,
            selection_binding,
            children,
            overlay,
            place,
            place_binding,
            slot_owner: _,
            accepts_slots: _,
            lifecycle,
        } = element;
        assert!(
            lifecycle.is_none(),
            "component lifecycle must be taken before the node is mounted"
        );
        let mut text_binding = None;
        let mut input_read = None;
        let mut disabled_when = element_disabled_when;
        let focusable = focusable || matches!(content, ElementContent::Input(_));
        let content = match content {
            ElementContent::Container
            | ElementContent::Slot { .. }
            | ElementContent::Fragment(_)
            | ElementContent::Flow(_) => NodeContent::Container,
            ElementContent::StaticText(text) => NodeContent::Text(text),
            ElementContent::DynamicText(binding) => {
                text_binding = Some(binding);
                NodeContent::Text("".into())
            }
            ElementContent::Image { source, fit } => {
                let (clip, load) = crate::image_load::open(source);
                let pixels = clip.frame_pixels(0);
                NodeContent::Image {
                    clip,
                    pixels,
                    fit,
                    elapsed: std::time::Duration::ZERO,
                    clock: None,
                    shown: 0,
                    load,
                }
            }
            ElementContent::Input(spec) => {
                disabled_when = disabled_when.or(spec.disabled_when);
                let (text, writer) = match spec.value {
                    InputValue::Fixed(text) => (text, None),
                    InputValue::Bound { read, write } => {
                        input_read = Some(read);
                        (String::new(), Some(write))
                    }
                };
                let edit = if spec.multiline {
                    TextEdit::from_lines(text)
                } else {
                    TextEdit::new(text)
                };
                let published = edit.text().to_owned();
                NodeContent::Input(InputField {
                    edit,
                    placeholder: spec.placeholder,
                    disabled: disabled || spec.disabled,
                    multiline: spec.multiline,
                    mask: spec.mask,
                    scroll_x: 0.0,
                    scroll_y: 0.0,
                    goal_x: None,
                    layout_sample: String::new(),
                    on_input: spec.on_input,
                    on_submit: spec.on_submit,
                    writer,
                    published,
                    swallow_text: None,
                })
            }
        };
        let initial_style = Style::inheriting(TextStyle::from_tokens(self.theme.tokens()));
        let node_disabled =
            disabled || matches!(&content, NodeContent::Input(field) if field.disabled);
        let node = Node {
            parent,
            children: Vec::new(),
            key,
            content,
            classes,
            dynamic_classes: Vec::new(),
            stylesheet,
            inline_style: style,
            dynamic_style: Default::default(),
            style: initial_style,
            interaction: cappy_style::InteractionState {
                disabled: node_disabled,
                ..cappy_style::InteractionState::default()
            },
            on_click,
            on_hover,
            focusable,
            focus_scope,
            focus_return: None,
            roving_active: None,
            disabled: node_disabled,
            overlay,
            anchor: place.unwrap_or_default(),
            on_key,
            on_text,
            on_ime,
            on_focus,
            on_pointer,
            on_dismiss,
            caret,
            selection,
            parts: Vec::new(),
            suppressed,
            alive: true,
            generation: 0,
            bindings: Vec::new(),
            scroll: cappy_layout::Point::default(),
            scroll_target: cappy_layout::Point::default(),
            rect: cappy_layout::Rect::ZERO,
            painted: cappy_layout::Rect::ZERO,
            clip: None,
            translation: cappy_layout::Point::default(),
            opacity: 1.0,
            background: None,
            scale: 1.0,
            rotation: 0.0,
            translation_tween: None,
            opacity_tween: None,
            background_tween: None,
            scale_tween: None,
            rotation_tween: None,
            shake: None,
            style_dirty: true,
            paint_dirty: true,
        };
        let id = self.insert_node(node);
        // The node flag alone does not run resolve. A mount during an update, such as
        // opening a branch, has to resolve before layout or a list window reads the
        // unresolved rect.
        self.style_dirty = true;
        let layout_style = self.layout_style_of(id);
        self.layout
            .insert(id, &layout_style)
            .expect("inserting a fresh layout node cannot fail");
        self.sync_text_leaf(id);
        self.sync_input_leaf(id);
        if let Some(binding) = text_binding {
            self.add_binding(id, BindingKind::Text(binding));
        }
        if let Some(binding) = input_read {
            self.add_binding(id, BindingKind::InputValue(binding));
        }
        if let Some(binding) = disabled_when {
            self.add_binding(id, BindingKind::Disabled(binding));
        }
        if let Some(binding) = caret_binding {
            self.add_binding(id, BindingKind::Caret(binding));
        }
        if let Some(binding) = selection_binding {
            self.add_binding(id, BindingKind::Selection(binding));
        }
        if let Some(binding) = class_binding {
            self.add_binding(id, BindingKind::Classes(binding));
        }
        if let Some(binding) = style_binding {
            self.add_binding(id, BindingKind::Style(binding));
        }
        if let Some(binding) = place_binding {
            self.add_binding(id, BindingKind::Place(binding));
        }
        let parts = self.adopt_children(id, children, suppressed);
        self.nodes[id.index()].parts = parts;
        self.reflatten(id);
        if !suppressed {
            self.arm_mounted_scope(id);
        }
        id
    }

    fn adopt_children(
        &mut self,
        parent: NodeId,
        children: Vec<Element>,
        suppressed: bool,
    ) -> Vec<ChildPart> {
        let mut parts = Vec::new();
        for child in children {
            parts.extend(self.adopt(parent, child, suppressed));
        }
        parts
    }

    fn adopt(&mut self, parent: NodeId, element: Element, suppressed: bool) -> Vec<ChildPart> {
        if element.lifecycle.is_some() {
            let (hooks, element) = self.open_component(element);
            let hooks = hooks.expect("lifecycle");
            let parts = self.adopt(parent, element, suppressed);
            return vec![self.close_component(hooks, parts).part()];
        }
        if let ElementContent::Fragment(children) = element.content {
            return self.adopt_children(parent, children, suppressed);
        }
        if let ElementContent::Flow(spec) = element.content {
            return vec![self.spawn_flow(parent, spec, suppressed).part()];
        }
        if matches!(element.content, ElementContent::Slot { .. }) {
            return Vec::new();
        }
        vec![ChildPart::Node(self.mount_node(
            element,
            Some(parent),
            suppressed,
        ))]
    }

    fn spawn_flow(&mut self, parent: NodeId, spec: FlowSpec, suppressed: bool) -> FlowId {
        let id = FlowId(u32::try_from(self.flows.len()).expect("too many flows"));
        self.flows.push(FlowState {
            parent,
            binding: BindingId::from_index(0),
            runtime: FlowRuntime::Busy,
            parts: Vec::new(),
            owner: None,
            ancestor_suppressed: suppressed,
            hidden: false,
            ready: false,
            disposed: false,
        });
        let binding = self.alloc_binding(parent, BindingKind::Flow(id));
        self.flows[id.index()].binding = binding;
        self.flows[id.index()].runtime = spec.into_runtime();
        self.sync_flow(id);
        id
    }

    pub(super) fn sync_flow(&mut self, id: FlowId) {
        let Some(flow) = self.flows.get(id.index()) else {
            return;
        };
        if flow.disposed {
            return;
        }
        let runtime = std::mem::replace(&mut self.flows[id.index()].runtime, FlowRuntime::Busy);
        match runtime {
            FlowRuntime::Show(state) => self.sync_show(id, state),
            FlowRuntime::Branch(state) => self.sync_branch(id, state),
            FlowRuntime::Repeat(runtime) => self.sync_repeat(id, runtime),
            FlowRuntime::Window(runtime) => self.sync_window(id, runtime),
            FlowRuntime::Busy => panic!("flow synced while it was already syncing"),
        }
    }

    fn sync_show(&mut self, id: FlowId, state: ShowRuntime) {
        let binding = self.flows[id.index()].binding;
        let visible = track(Observer::new(&self.queue, binding), || (state.when)());
        let ready = self.flows[id.index()].ready;
        let was_hidden = self.flows[id.index()].hidden;
        if ready && was_hidden == !visible {
            self.flows[id.index()].runtime = FlowRuntime::Show(state);
            return;
        }
        self.flows[id.index()].hidden = !visible;
        self.flows[id.index()].runtime = FlowRuntime::Show(state);
        if ready {
            self.propagate_flow(id);
            self.refresh_pointer();
            return;
        }
        let guard = enter_owner(&self.queue.runtime);
        let owner = guard.id();
        let element = match &self.flows[id.index()].runtime {
            FlowRuntime::Show(state) => (state.view)(),
            _ => unreachable!("show runtime was just stored"),
        };
        drop(guard);
        let parent = self.flows[id.index()].parent;
        self.flows[id.index()].owner = Some(owner);
        self.flows[id.index()].ready = true;
        let suppressed = self.flow_is_suppressed(id);
        let parts = self.adopt(parent, element, suppressed);
        self.flows[id.index()].parts = parts;
        self.reflatten(parent);
    }

    fn sync_branch(&mut self, id: FlowId, mut state: BranchRuntime) {
        let binding = self.flows[id.index()].binding;
        let choice = track(Observer::new(&self.queue, binding), || (state.when)());
        if self.flows[id.index()].ready && state.current == Some(choice) {
            self.flows[id.index()].runtime = FlowRuntime::Branch(state);
            return;
        }
        let parent = self.flows[id.index()].parent;
        let old_parts = std::mem::take(&mut self.flows[id.index()].parts);
        let old_owner = self.flows[id.index()].owner.take();
        self.reflatten(parent);
        self.dispose_parts(old_parts);
        if let Some(owner) = old_owner {
            dispose_owner(&self.queue.runtime, owner);
        }
        let guard = enter_owner(&self.queue.runtime);
        let owner = guard.id();
        let element = if choice {
            (state.then_branch)()
        } else {
            (state.else_branch)()
        };
        drop(guard);
        state.current = Some(choice);
        self.flows[id.index()].runtime = FlowRuntime::Branch(state);
        self.flows[id.index()].owner = Some(owner);
        self.flows[id.index()].ready = true;
        let suppressed = self.flow_is_suppressed(id);
        let parts = self.adopt(parent, element, suppressed);
        self.flows[id.index()].parts = parts;
        self.reflatten(parent);
        self.refresh_pointer();
    }

    fn sync_repeat(&mut self, id: FlowId, mut runtime: Box<dyn RepeatRuntime>) {
        runtime.reconcile(self, id);
        self.flows[id.index()].runtime = FlowRuntime::Repeat(runtime);
        self.flows[id.index()].ready = true;
    }

    fn sync_window(&mut self, id: FlowId, mut runtime: WindowRuntime) {
        runtime.reconcile(self, id);
        self.flows[id.index()].runtime = FlowRuntime::Window(runtime);
        self.flows[id.index()].ready = true;
    }

    fn flow_is_suppressed(&self, id: FlowId) -> bool {
        let flow = &self.flows[id.index()];
        flow.ancestor_suppressed || flow.hidden
    }

    fn propagate_flow(&mut self, id: FlowId) {
        let suppressed = self.flow_is_suppressed(id);
        let parts = self.flows[id.index()].parts.clone();
        for part in parts {
            self.suppress_part(part, suppressed);
        }
    }

    fn suppress_part(&mut self, part: ChildPart, suppressed: bool) {
        match part {
            ChildPart::Node(node) => self.suppress_node_tree(node, suppressed),
            ChildPart::Component(id) => {
                let Some(parts) = self.component_record(id).map(|record| record.parts.clone())
                else {
                    return;
                };
                for part in parts {
                    self.suppress_part(part, suppressed);
                }
            }
            ChildPart::Flow(raw) => {
                let child = FlowId(raw);
                if self.flows[child.index()].disposed {
                    return;
                }
                self.flows[child.index()].ancestor_suppressed = suppressed;
                self.propagate_flow(child);
            }
        }
    }

    fn suppress_node_tree(&mut self, id: NodeId, suppressed: bool) {
        if !self.nodes[id.index()].alive {
            return;
        }
        if self.nodes[id.index()].suppressed != suppressed {
            self.nodes[id.index()].suppressed = suppressed;
            self.roving_visibility(id, !suppressed);
            if suppressed {
                self.cancel_motion(id);
            }
            let layout_style = self.layout_style_of(id);
            self.layout
                .set_style(id, &layout_style)
                .expect("layout tree is out of sync with the node tree");
            self.damage.add(self.nodes[id.index()].painted);
            self.damage.add(self.nodes[id.index()].rect);
            self.mark_style_dirty(id);
            self.display_list_stale = true;
            self.layout_dirty = true;
        }
        let parts = self.nodes[id.index()].parts.clone();
        for part in parts {
            self.suppress_part(part, suppressed);
        }
    }

    fn reflatten(&mut self, id: NodeId) {
        if !self.nodes[id.index()].alive {
            return;
        }
        let children = self.flatten_node(id);
        let layout_children: Vec<NodeId> = children
            .iter()
            .copied()
            .filter(|child| !self.nodes[child.index()].overlay)
            .collect();
        self.layout
            .set_children(id, &layout_children)
            .expect("layout children should still be mounted");
        self.nodes[id.index()].children = children;
        self.overlay_order_dirty.set(true);
        self.layout_dirty = true;
        self.display_list_stale = true;
    }

    fn flatten_node(&self, id: NodeId) -> Vec<NodeId> {
        let parts = self.nodes[id.index()].parts.clone();
        let mut out = Vec::new();
        self.extend_parts(&parts, &mut out);
        out
    }

    fn extend_parts(&self, parts: &[ChildPart], out: &mut Vec<NodeId>) {
        for part in parts {
            match *part {
                ChildPart::Node(id) => out.push(id),
                ChildPart::Component(id) => {
                    let nested = self.component_record(id).map(|record| record.parts.clone());
                    if let Some(nested) = nested {
                        self.extend_parts(&nested, out);
                    }
                }
                ChildPart::Flow(raw) => {
                    let flow = FlowId(raw);
                    if self
                        .flows
                        .get(flow.index())
                        .is_some_and(|flow| !flow.disposed)
                    {
                        let nested = self.flows[flow.index()].parts.clone();
                        self.extend_parts(&nested, out);
                    }
                }
            }
        }
    }

    pub(super) fn dispose_tree(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        if let Some(id) = self.root_component.take() {
            self.dispose_component(id);
            self.refresh_pointer();
            return;
        }
        let root = self.root;
        if self.nodes[root.index()].alive {
            self.dispose_node(root);
        }
        self.refresh_pointer();
    }

    fn dispose_parts(&mut self, parts: Vec<ChildPart>) {
        for part in parts {
            match part {
                ChildPart::Node(id) => self.dispose_node(id),
                ChildPart::Flow(raw) => self.dispose_flow(FlowId(raw)),
                ChildPart::Component(id) => self.dispose_component(id),
            }
        }
    }

    fn dispose_component(&mut self, id: ComponentId) {
        let (mut hooks, parts) = {
            let Some(record) = self.components.get_mut(id.index()) else {
                return;
            };
            if record.disposed || record.generation != id.generation() {
                return;
            }
            record.disposed = true;
            (
                std::mem::take(&mut record.hooks),
                std::mem::take(&mut record.parts),
            )
        };
        self.ticking.retain(|tick| *tick != id);
        if self.root_component == Some(id) {
            self.root_component = None;
        }
        self.fire_hooks(&mut hooks.before_unmount);
        self.dispose_parts(parts);
        self.fire_hooks(&mut hooks.unmounted);
        self.free_components.push(id.index() as u32);
    }

    fn dispose_flow(&mut self, id: FlowId) {
        let Some(flow) = self.flows.get_mut(id.index()) else {
            return;
        };
        if flow.disposed {
            return;
        }
        flow.disposed = true;
        let parent = flow.parent;
        let binding = flow.binding;
        let runtime = std::mem::replace(&mut flow.runtime, FlowRuntime::Busy);
        match runtime {
            FlowRuntime::Repeat(mut runtime) => {
                self.flows[id.index()].parts.clear();
                self.reflatten(parent);
                runtime.dispose(self);
            }
            FlowRuntime::Window(mut runtime) => {
                self.flows[id.index()].parts.clear();
                self.reflatten(parent);
                runtime.dispose(self);
            }
            _ => {
                let parts = std::mem::take(&mut self.flows[id.index()].parts);
                let owner = self.flows[id.index()].owner.take();
                self.reflatten(parent);
                self.dispose_parts(parts);
                if let Some(owner) = owner {
                    dispose_owner(&self.queue.runtime, owner);
                }
            }
        }
        self.retire_binding(binding);
    }

    fn dispose_node(&mut self, id: NodeId) {
        self.prepare_scope_dispose(id);
        let Some(node) = self.nodes.get(id.index()) else {
            return;
        };
        if !node.alive || node.generation != id.generation() {
            return;
        }
        self.cancel_timer(id);
        let parts = std::mem::take(&mut self.nodes[id.index()].parts);
        self.dispose_parts(parts);
        let bindings = std::mem::take(&mut self.nodes[id.index()].bindings);
        for binding in bindings {
            self.retire_binding(binding);
        }
        let painted = self.nodes[id.index()].painted;
        let rect = self.nodes[id.index()].rect;
        let clip = self.nodes[id.index()].clip;
        self.damage_clipped(painted, clip);
        self.damage_clipped(rect, clip);
        let _ = self.layout.remove(id);
        let geometry_key = self.nodes[id.index()].key.clone();
        let node = &mut self.nodes[id.index()];
        node.alive = false;
        node.children.clear();
        node.key = None;
        node.on_click = None;
        node.on_hover = None;
        node.on_key = None;
        node.on_text = None;
        node.on_ime = None;
        node.on_focus = None;
        node.on_pointer = None;
        node.on_dismiss = None;
        node.focusable = false;
        node.focus_scope = None;
        node.focus_return = None;
        node.roving_active = None;
        node.overlay = false;
        node.anchor = cappy_layout::Point::default();
        node.caret = None;
        node.selection = None;
        node.content = NodeContent::Container;
        node.suppressed = true;
        node.paint_dirty = false;
        node.style_dirty = false;
        node.clip = None;
        node.translation = cappy_layout::Point::default();
        node.opacity = 1.0;
        node.background = None;
        node.scale = 1.0;
        node.rotation = 0.0;
        node.translation_tween = None;
        node.opacity_tween = None;
        node.background_tween = None;
        node.scale_tween = None;
        node.rotation_tween = None;
        node.shake = None;
        node.parent = None;
        if self.pressed == Some(id) {
            self.pressed = None;
        }
        if self.focused == Some(id) {
            self.focused = None;
            self.caret_deadline = None;
        }
        if self.pointer_capture == Some(id) {
            self.pointer_capture = None;
        }
        if self
            .button_capture
            .is_some_and(|(_, captured)| captured == id)
        {
            self.button_capture = None;
        }
        if self.pending_blur == Some(id) {
            self.pending_blur = None;
        }
        self.pending_dismiss.retain(|pending| *pending != id);
        if self
            .scrollbar_drag
            .as_ref()
            .is_some_and(|drag| drag.id == id)
        {
            self.scrollbar_drag = None;
        }
        self.display_list_stale = true;
        if let Some(key) = geometry_key {
            self.board.forget(&key, id);
        }
        self.free_nodes.push(id.index() as u32);
    }

    pub(super) fn reclaim_disposed_flows(&mut self) {
        while self.flows.last().is_some_and(|flow| flow.disposed) {
            self.flows.pop();
        }
        self.flows.shrink_to_fit();
    }

    fn refresh_pointer(&mut self) {
        if let Some(drag) = self.scrollbar_drag
            && (!self.is_current(drag.id) || self.nodes[drag.id.index()].suppressed)
        {
            self.scrollbar_drag = None;
        }
        if self.scrollbar_drag.is_some() {
            return;
        }
        if self.pointer.is_some() {
            self.refresh_hover();
        }
        let Some(id) = self.pressed else {
            return;
        };
        if !self.is_current(id) {
            self.pressed = None;
            return;
        }
        let node = &self.nodes[id.index()];
        if !node.alive || node.suppressed {
            self.pressed = None;
            if node.alive {
                self.set_interaction(id, |state| state.pressed = false);
            }
        }
    }
}

impl<T, K> RepeatRuntime for ListRuntime<T, K>
where
    T: Clone + PartialEq + 'static,
    K: ToOwned + Eq + Hash + ?Sized + 'static,
    K::Owned: Clone + Eq + Hash + Borrow<K> + 'static,
{
    fn reconcile(&mut self, ui: &mut Ui, flow: FlowId) {
        let binding = ui.flows[flow.index()].binding;
        let mut seen = Vec::new();
        let mut updates = Vec::new();
        let mut fresh = Vec::new();
        let mut used = HashSet::new();
        let mut duplicate = false;
        {
            let rows = &self.rows;
            let index = &self.index;
            let key_of = &self.key_of;
            let read = &self.read;
            track(Observer::new(&ui.queue, binding), || {
                read(&mut |item| {
                    if duplicate {
                        return;
                    }
                    let key = key_of(item);
                    if let Some(&row_index) = index.get(key) {
                        if !used.insert(row_index) {
                            duplicate = true;
                            return;
                        }
                        if rows[row_index]
                            .value
                            .with_untracked(|current| current != item)
                        {
                            updates.push((row_index, item.clone()));
                        }
                        seen.push(Seen::Keep(row_index));
                    } else {
                        let owned = key.to_owned();
                        if fresh.iter().any(|(existing, _)| existing == &owned) {
                            duplicate = true;
                            return;
                        }
                        fresh.push((owned, item.clone()));
                        seen.push(Seen::Fresh(fresh.len() - 1));
                    }
                });
            });
        }
        if duplicate {
            panic!("duplicate key in for");
        }
        for (row_index, value) in updates {
            self.rows[row_index].value.set(value);
        }
        let structure_same = fresh.is_empty()
            && used.len() == self.rows.len()
            && seen
                .iter()
                .enumerate()
                .all(|(position, slot)| matches!(slot, Seen::Keep(index) if *index == position));
        if structure_same {
            return;
        }

        let parent = ui.flows[flow.index()].parent;
        ui.flows[flow.index()].parts.clear();
        ui.reflatten(parent);
        let mut old_rows: Vec<Option<ListRow<T, K::Owned>>> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(Some)
            .collect();
        for (index, row) in old_rows.iter_mut().enumerate() {
            if !used.contains(&index)
                && let Some(row) = row.take()
            {
                ui.dispose_parts(row.parts);
                dispose_owner(&ui.queue.runtime, row.owner);
            }
        }
        let suppressed = ui.flow_is_suppressed(flow);
        let mut new_rows = Vec::with_capacity(seen.len());
        let mut parts = Vec::new();
        let mut fresh_slots: Vec<Option<(K::Owned, T)>> = fresh.into_iter().map(Some).collect();
        for slot in seen {
            match slot {
                Seen::Keep(index) => {
                    let row = old_rows[index].take().expect("kept row is still here");
                    parts.extend(row.parts.iter().copied());
                    new_rows.push(row);
                }
                Seen::Fresh(index) => {
                    let (key, value) = fresh_slots[index].take().expect("fresh row is still here");
                    let row =
                        mount_list_row(ui, parent, self.view.as_ref(), key, value, suppressed);
                    parts.extend(row.parts.iter().copied());
                    new_rows.push(row);
                }
            }
        }
        self.rows = new_rows;
        self.index.clear();
        for (index, row) in self.rows.iter().enumerate() {
            self.index.insert(row.key.clone(), index);
        }
        ui.flows[flow.index()].parts = parts;
        ui.reflatten(parent);
        ui.refresh_pointer();
    }

    fn dispose(&mut self, ui: &mut Ui) {
        for row in self.rows.drain(..) {
            ui.dispose_parts(row.parts);
            dispose_owner(&ui.queue.runtime, row.owner);
        }
        self.index.clear();
    }
}

enum Seen {
    Keep(usize),
    Fresh(usize),
}

fn mount_list_row<T, K>(
    ui: &mut Ui,
    parent: NodeId,
    view: &dyn Fn(Signal<T>) -> Element,
    key: K,
    value: T,
    suppressed: bool,
) -> ListRow<T, K>
where
    T: 'static,
{
    let guard = enter_owner(&ui.queue.runtime);
    let owner = guard.id();
    let signal = Signal::create(&ui.queue.runtime, value, false);
    let element = view(signal.clone());
    drop(guard);
    let parts = ui.adopt(parent, element, suppressed);
    ListRow {
        key,
        value: signal,
        parts,
        owner,
    }
}

/// Rows mounted for the current scroll window. The index is the row's identity.
pub(crate) struct WindowRuntime {
    scroller: String,
    count: Box<dyn Fn() -> usize>,
    extent: f32,
    buffer: usize,
    view: Box<dyn Fn(usize) -> Element>,
    rows: Vec<WindowRow>,
    before: Option<NodeId>,
    after: Option<NodeId>,
    /// The scrollport has published a rect. Until then the trailing spacer holds the full height.
    placed: bool,
    start: usize,
    end: usize,
    len: usize,
}

struct WindowRow {
    index: usize,
    parts: Vec<ChildPart>,
    owner: crate::reactive::OwnerId,
}

#[derive(Clone, Copy)]
struct WindowSpan {
    start: usize,
    end: usize,
    before: f32,
    after: f32,
}

impl WindowRuntime {
    fn reconcile(&mut self, ui: &mut Ui, flow: FlowId) {
        let binding = ui.flows[flow.index()].binding;
        let queue_runtime = Rc::clone(&ui.queue.runtime);
        let (count, scroll, rect) = track(Observer::new(&ui.queue, binding), || {
            let count = (self.count)();
            let scroll = ui.board.read_scroll(&queue_runtime, &self.scroller);
            let rect = ui.board.read_rect(&queue_runtime, &self.scroller);
            (count, scroll, rect)
        });
        let placed = rect.is_some();
        let span = window_span(
            count,
            self.extent,
            rect.map(|rect| rect.height).unwrap_or(0.0),
            scroll.map(|offset| offset.y).unwrap_or(0.0),
            self.buffer,
            placed,
        );
        // Scroll eases every frame. The same index range must not allocate or relayout.
        if self.before.is_some()
            && self.placed == placed
            && self.len == count
            && self.start == span.start
            && self.end == span.end
        {
            return;
        }
        let rows_held = self.before.is_some()
            && self.placed == placed
            && self.start == span.start
            && self.end == span.end;
        self.placed = placed;
        self.len = count;
        self.start = span.start;
        self.end = span.end;
        if rows_held {
            assign_height(ui, self.before.expect("leading spacer"), span.before);
            assign_height(ui, self.after.expect("trailing spacer"), span.after);
            return;
        }
        self.rebuild(ui, flow, span);
    }

    fn rebuild(&mut self, ui: &mut Ui, flow: FlowId, span: WindowSpan) {
        let parent = ui.flows[flow.index()].parent;
        let suppressed = ui.flow_is_suppressed(flow);
        ui.flows[flow.index()].parts.clear();
        ui.reflatten(parent);
        self.ensure_spacers(ui, parent, span, suppressed);
        let extent = self.extent;
        let mut old = std::mem::take(&mut self.rows).into_iter().peekable();
        let mut rows = Vec::new();
        for index in span.start..span.end {
            while old.peek().is_some_and(|row| row.index < index) {
                release_row(ui, old.next().expect("row"));
            }
            if old.peek().is_some_and(|row| row.index == index) {
                rows.push(old.next().expect("row"));
            } else {
                rows.push(mount_window_row(
                    ui,
                    parent,
                    self.view.as_ref(),
                    index,
                    extent,
                    suppressed,
                ));
            }
        }
        for row in old {
            release_row(ui, row);
        }
        self.rows = rows;
        self.link(ui, flow, parent);
    }

    fn ensure_spacers(&mut self, ui: &mut Ui, parent: NodeId, span: WindowSpan, suppressed: bool) {
        if let (Some(before), Some(after)) = (self.before, self.after) {
            assign_height(ui, before, span.before);
            assign_height(ui, after, span.after);
            return;
        }
        self.before = Some(mount_spacer(ui, parent, span.before, suppressed));
        self.after = Some(mount_spacer(ui, parent, span.after, suppressed));
    }

    fn link(&self, ui: &mut Ui, flow: FlowId, parent: NodeId) {
        let before = self.before.expect("leading spacer");
        let after = self.after.expect("trailing spacer");
        let mut parts = Vec::with_capacity(self.rows.len() + 2);
        parts.push(ChildPart::Node(before));
        for row in &self.rows {
            parts.extend(row.parts.iter().copied());
        }
        parts.push(ChildPart::Node(after));
        ui.flows[flow.index()].parts = parts;
        ui.reflatten(parent);
        ui.refresh_pointer();
    }

    fn dispose(&mut self, ui: &mut Ui) {
        for row in self.rows.drain(..) {
            release_row(ui, row);
        }
        if let Some(id) = self.before.take() {
            ui.dispose_node(id);
        }
        if let Some(id) = self.after.take() {
            ui.dispose_node(id);
        }
    }
}

/// Visible rows are `[floor(offset / extent), ceil((offset + viewport) / extent))`, then `buffer`
/// rows on each side. The spacers make the sum `count * extent`.
fn window_span(
    count: usize,
    extent: f32,
    viewport: f32,
    offset: f32,
    buffer: usize,
    placed: bool,
) -> WindowSpan {
    let total = if extent.is_finite() && extent > 0.0 {
        (count as f32 * extent).max(0.0)
    } else {
        0.0
    };
    if !placed || count == 0 || total == 0.0 {
        return WindowSpan {
            start: 0,
            end: 0,
            before: 0.0,
            after: total,
        };
    }
    let offset = if offset.is_finite() {
        offset.max(0.0)
    } else {
        0.0
    };
    let viewport = if viewport.is_finite() {
        viewport.max(0.0)
    } else {
        0.0
    };
    let mut start = finite_index((offset / extent).floor()).saturating_sub(buffer);
    let mut end = finite_index(((offset + viewport) / extent).ceil()).saturating_add(buffer);
    if start > count {
        start = count;
    }
    if end > count {
        end = count;
    }
    if start > end {
        start = end;
    }
    let before = start as f32 * extent;
    let covered = (end - start) as f32 * extent;
    WindowSpan {
        start,
        end,
        before,
        after: (total - before - covered).max(0.0),
    }
}

fn finite_index(value: f32) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= usize::MAX as f32 {
        return usize::MAX;
    }
    value as usize
}

fn spacer_element(height: f32) -> Element {
    div().style(StyleRefinement::default().height(height).flex_shrink(0.0))
}

fn mount_spacer(ui: &mut Ui, parent: NodeId, height: f32, suppressed: bool) -> NodeId {
    let parts = ui.adopt(parent, spacer_element(height), suppressed);
    let &[ChildPart::Node(id)] = parts.as_slice() else {
        panic!("spacer mounts one node");
    };
    id
}

fn assign_height(ui: &mut Ui, id: NodeId, height: f32) {
    let height = Dimension::from(height);
    if ui.nodes[id.index()].inline_style.height == Some(height) {
        return;
    }
    ui.nodes[id.index()].inline_style.height = Some(height);
    ui.mark_style_dirty(id);
}

fn mount_window_row(
    ui: &mut Ui,
    parent: NodeId,
    view: &dyn Fn(usize) -> Element,
    index: usize,
    extent: f32,
    suppressed: bool,
) -> WindowRow {
    let guard = enter_owner(&ui.queue.runtime);
    let owner = guard.id();
    let element = div()
        .style(
            StyleRefinement::default()
                .direction(FlexDirection::Column)
                .height(extent)
                .flex_shrink(0.0),
        )
        .child(view(index));
    drop(guard);
    let parts = ui.adopt(parent, element, suppressed);
    WindowRow {
        index,
        parts,
        owner,
    }
}

fn release_row(ui: &mut Ui, row: WindowRow) {
    ui.dispose_parts(row.parts);
    dispose_owner(&ui.queue.runtime, row.owner);
}
