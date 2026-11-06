//! Reactive bindings: closures whose result is written into a node whenever a signal they read
//! changes.

use std::sync::Arc;

use cappy_style::Theme;

use super::flow::FlowId;
use super::{NodeId, Ui};
use crate::element::{ClassBinding, StyleBinding, TextBinding};
use crate::reactive::{detach_binding, BindingId, Observer, track};

pub(super) enum BindingKind {
    Text(TextBinding),
    Classes(ClassBinding),
    Style(StyleBinding),
    Theme,
    Flow(FlowId),
    Disposed,
}

pub(super) struct Binding {
    pub(super) node: NodeId,
    pub(super) kind: BindingKind,
    pub(super) disposed: bool,
}

/// Upper bound on binding passes per update, so bindings that write signals they read
/// cannot spin forever; anything left stays queued for the next update.
const MAX_BINDING_PASSES: usize = 16;

impl Ui {
    pub(super) fn alloc_binding(&mut self, node: NodeId, kind: BindingKind) -> BindingId {
        let id = BindingId(u32::try_from(self.bindings.len()).expect("too many bindings"));
        self.bindings.push(Binding {
            node,
            kind,
            disposed: false,
        });
        self.nodes[node.index()].bindings.push(id);
        id
    }

    pub(super) fn add_binding(&mut self, node: NodeId, kind: BindingKind) {
        let id = self.alloc_binding(node, kind);
        self.evaluate_binding(id);
    }

    /// Detaches subscribers and drops the closure. A queued run then does nothing.
    pub(super) fn retire_binding(&mut self, id: BindingId) {
        let binding = &mut self.bindings[id.0 as usize];
        if binding.disposed {
            return;
        }
        binding.disposed = true;
        binding.kind = BindingKind::Disposed;
        detach_binding(&self.queue, id);
    }

    pub(super) fn process_bindings(&mut self) {
        for _ in 0..MAX_BINDING_PASSES {
            let pending = self.queue.take();
            if pending.is_empty() {
                return;
            }
            for binding in pending {
                self.evaluate_binding(binding);
            }
        }
    }

    fn evaluate_binding(&mut self, id: BindingId) {
        if self.bindings[id.0 as usize].disposed {
            return;
        }
        let observer = Observer::new(&self.queue, id);
        let node = self.bindings[id.0 as usize].node;
        enum Step {
            Text(String),
            Classes(Vec<cappy_style::ClassName>),
            Style(Box<cappy_style::StyleRefinement>),
            Theme(Arc<Theme>),
            Flow(FlowId),
        }
        let step = match &self.bindings[id.0 as usize].kind {
            BindingKind::Text(compute) => Step::Text(track(observer, compute)),
            BindingKind::Classes(compute) => Step::Classes(track(observer, compute)),
            BindingKind::Style(compute) => Step::Style(Box::new(track(observer, compute))),
            BindingKind::Theme => {
                let theme_signal = self.theme_signal;
                Step::Theme(track(observer, || theme_signal.get()))
            }
            BindingKind::Flow(flow) => Step::Flow(*flow),
            BindingKind::Disposed => return,
        };
        match step {
            Step::Text(text) => self.set_text_content(node, text),
            Step::Classes(classes) => {
                let target = &mut self.nodes[node.index()];
                if target.dynamic_classes != classes {
                    target.dynamic_classes = classes;
                    self.mark_style_dirty(node);
                }
            }
            Step::Style(style) => {
                let target = &mut self.nodes[node.index()];
                if target.dynamic_style != *style {
                    target.dynamic_style = *style;
                    self.mark_style_dirty(node);
                }
            }
            Step::Theme(theme) => self.apply_theme(theme),
            Step::Flow(flow) => self.sync_flow(flow),
        }
    }

    fn apply_theme(&mut self, theme: Arc<Theme>) {
        if Arc::ptr_eq(&theme, &self.theme) {
            return;
        }
        self.theme = theme;
        for index in 0..self.nodes.len() {
            self.mark_style_dirty(NodeId(index as u32));
        }
        self.damage.add_full();
    }
}
