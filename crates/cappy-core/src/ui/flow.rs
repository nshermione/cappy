//! `show`, `if` / `else` and `for`. Each block is one binding on the parent. `show` hides.
//! `if` and `for` dispose what they drop, walking every nested block the same way.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use cappy_style::{Style, TextStyle};

use super::Ui;
use super::bindings::BindingKind;
use super::node::{ChildPart, Node, NodeContent, NodeId};
use crate::element::{Element, ElementContent, div};
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
        }
    }
}

impl Ui {
    pub(super) fn mount(&mut self, element: Element, parent: Option<NodeId>) -> NodeId {
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
            children,
            slot_owner: _,
            accepts_slots: _,
        } = element;
        let (content, text_binding) = match content {
            ElementContent::Container
            | ElementContent::Slot { .. }
            | ElementContent::Fragment(_)
            | ElementContent::Flow(_) => (NodeContent::Container, None),
            ElementContent::StaticText(text) => (NodeContent::Text(text), None),
            ElementContent::DynamicText(binding) => (NodeContent::Text("".into()), Some(binding)),
            ElementContent::Image { pixels, fit } => (NodeContent::Image { pixels, fit }, None),
        };
        let initial_style = Style::inheriting(TextStyle::from_tokens(self.theme.tokens()));
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
            interaction: Default::default(),
            on_click,
            on_hover,
            parts: Vec::new(),
            suppressed,
            alive: true,
            generation: 0,
            bindings: Vec::new(),
            scroll: cappy_layout::Point::default(),
            scroll_target: cappy_layout::Point::default(),
            rect: cappy_layout::Rect::ZERO,
            painted: cappy_layout::Rect::ZERO,
            style_dirty: true,
            paint_dirty: true,
        };
        let id = self.insert_node(node);
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
        let parts = self.adopt_children(id, children, suppressed);
        self.nodes[id.index()].parts = parts;
        self.reflatten(id);
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
        self.layout
            .set_children(id, &children)
            .expect("layout children should still be mounted");
        self.nodes[id.index()].children = children;
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
            }
        }
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
        let Some(node) = self.nodes.get(id.index()) else {
            return;
        };
        if !node.alive || node.generation != id.generation() {
            return;
        }
        let parts = std::mem::take(&mut self.nodes[id.index()].parts);
        self.dispose_parts(parts);
        let bindings = std::mem::take(&mut self.nodes[id.index()].bindings);
        for binding in bindings {
            self.retire_binding(binding);
        }
        self.damage.add(self.nodes[id.index()].painted);
        self.damage.add(self.nodes[id.index()].rect);
        let _ = self.layout.remove(id);
        let node = &mut self.nodes[id.index()];
        node.alive = false;
        node.children.clear();
        node.key = None;
        node.on_click = None;
        node.on_hover = None;
        node.content = NodeContent::Container;
        node.suppressed = true;
        node.paint_dirty = false;
        node.style_dirty = false;
        node.parent = None;
        if self.pressed == Some(id) {
            self.pressed = None;
        }
        if self
            .scrollbar_drag
            .as_ref()
            .is_some_and(|drag| drag.id == id)
        {
            self.scrollbar_drag = None;
        }
        self.display_list_stale = true;
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
