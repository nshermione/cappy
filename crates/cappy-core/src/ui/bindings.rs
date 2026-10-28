//! Reactive bindings: closures whose result is written into a node whenever a signal they read
//! changes.

use std::sync::Arc;

use cappy_style::{ClassName, StyleRefinement, Theme};

use super::{NodeId, Ui};
use crate::element::{ClassBinding, StyleBinding, TextBinding};
use crate::reactive::{BindingId, Observer, track};

pub(super) enum BindingKind {
    Text(TextBinding),
    Classes(ClassBinding),
    Style(StyleBinding),
    Theme,
}

pub(super) struct Binding {
    pub(super) node: NodeId,
    pub(super) kind: BindingKind,
}

enum BindingValue {
    Text(String),
    Classes(Vec<ClassName>),
    Style(Box<StyleRefinement>),
    Theme(Arc<Theme>),
}

/// Upper bound on binding passes per update, so bindings that write signals they read
/// cannot spin forever; anything left stays queued for the next update.
const MAX_BINDING_PASSES: usize = 16;

impl Ui {
    pub(super) fn add_binding(&mut self, node: NodeId, kind: BindingKind) {
        let id = BindingId(u32::try_from(self.bindings.len()).expect("too many bindings"));
        self.bindings.push(Binding { node, kind });
        self.evaluate_binding(id);
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
        let observer = Observer::new(&self.queue, id);
        let binding = &self.bindings[id.0 as usize];
        let node = binding.node;
        let value = match &binding.kind {
            BindingKind::Text(compute) => BindingValue::Text(track(observer, compute)),
            BindingKind::Classes(compute) => BindingValue::Classes(track(observer, compute)),
            BindingKind::Style(compute) => BindingValue::Style(Box::new(track(observer, compute))),
            BindingKind::Theme => {
                let theme_signal = self.theme_signal;
                BindingValue::Theme(track(observer, || theme_signal.get()))
            }
        };
        match value {
            BindingValue::Text(text) => self.set_text_content(node, text),
            BindingValue::Classes(classes) => {
                let target = &mut self.nodes[node.index()];
                if target.dynamic_classes != classes {
                    target.dynamic_classes = classes;
                    self.mark_style_dirty(node);
                }
            }
            BindingValue::Style(style) => {
                let target = &mut self.nodes[node.index()];
                if target.dynamic_style != *style {
                    target.dynamic_style = *style;
                    self.mark_style_dirty(node);
                }
            }
            BindingValue::Theme(theme) => self.apply_theme(theme),
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
