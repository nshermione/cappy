//! The style, layout and paint passes run by [`Ui::update`].

use std::sync::Arc;

use cappy_layout::{Point, TextLeaf};
use cappy_style::{Display, Extent, LayoutStyle, TextStyle};

use super::{NodeId, Ui};
use crate::paint::{Primitive, QuadPrimitive, TextPrimitive};

const LAYOUT_OUT_OF_SYNC: &str = "layout tree is out of sync with the node tree";

impl Ui {
    pub(super) fn mark_style_dirty(&mut self, node: NodeId) {
        self.nodes[node.index()].style_dirty = true;
        self.style_dirty = true;
    }

    pub(super) fn mark_paint_dirty(&mut self, node: NodeId) {
        self.nodes[node.index()].paint_dirty = true;
        self.paint_dirty = true;
    }

    pub(super) fn set_text_content(&mut self, node: NodeId, text: String) {
        let target = &mut self.nodes[node.index()];
        if target.text() == Some(text.as_str()) {
            return;
        }
        target.content = super::NodeContent::Text(Arc::from(text));
        self.sync_text_leaf(node);
        self.mark_paint_dirty(node);
    }

    /// Pushes a text node's content and font metrics to the layout tree.
    pub(super) fn sync_text_leaf(&mut self, node: NodeId) {
        let target = &self.nodes[node.index()];
        let Some(text) = target.text() else {
            return;
        };
        let leaf = text_leaf(text, &target.style.text);
        self.layout
            .set_text(node, Some(leaf))
            .expect(LAYOUT_OUT_OF_SYNC);
        self.layout_dirty = true;
    }

    /// The root fills the viewport unless its style gives it an explicit size.
    pub(super) fn layout_style_of(&self, node: NodeId) -> LayoutStyle {
        let mut style = self.nodes[node.index()].style.layout.clone();
        if node == self.root {
            for extent in [&mut style.width, &mut style.height] {
                if *extent == Extent::Auto {
                    *extent = Extent::Percent(100.0);
                }
            }
        }
        style
    }

    /// Re-resolves dirty styles top-down and records what each change invalidates.
    pub(super) fn resolve_styles(&mut self) {
        if !std::mem::take(&mut self.style_dirty) {
            return;
        }
        let mut stack: Vec<(NodeId, Option<TextStyle>, bool)> = vec![(self.root, None, false)];
        while let Some((id, parent_text, parent_text_changed)) = stack.pop() {
            let mut text_changed = false;
            if self.nodes[id.index()].style_dirty || parent_text_changed {
                text_changed = self.restyle(id, parent_text.as_ref());
            }
            let node = &self.nodes[id.index()];
            for child in node.children.iter().rev() {
                stack.push((*child, Some(node.style.text.clone()), text_changed));
            }
        }
    }

    /// Resolves one node's style. Returns whether its inherited text style changed.
    fn restyle(&mut self, id: NodeId, parent_text: Option<&TextStyle>) -> bool {
        let node = &self.nodes[id.index()];
        let resolved = self.theme.resolve(
            parent_text,
            &node.all_classes(),
            &node.effective_inline_style(),
            node.interaction,
        );
        let change = resolved.diff(&node.style);
        let is_text = node.is_text();
        let node = &mut self.nodes[id.index()];
        node.style = resolved;
        node.style_dirty = false;
        if change.layout {
            let layout_style = self.layout_style_of(id);
            self.layout
                .set_style(id, &layout_style)
                .expect(LAYOUT_OUT_OF_SYNC);
            self.layout_dirty = true;
            if is_text {
                self.sync_text_leaf(id);
            }
        }
        if change.paint {
            self.mark_paint_dirty(id);
        }
        change.inherited
    }

    /// Recomputes layout when needed and damages every node whose rectangle moved.
    pub(super) fn run_layout(&mut self) {
        if !std::mem::take(&mut self.layout_dirty) {
            return;
        }
        let viewport = self.logical_viewport();
        self.layout
            .compute(self.root, viewport, self.measurer.as_mut())
            .expect(LAYOUT_OUT_OF_SYNC);
        let mut stack = vec![(self.root, Point::default())];
        while let Some((id, origin)) = stack.pop() {
            let rect = self
                .layout
                .local_rect(id)
                .expect(LAYOUT_OUT_OF_SYNC)
                .translate(origin);
            let node = &mut self.nodes[id.index()];
            if node.rect != rect {
                self.damage.add(node.rect);
                self.damage.add(rect);
                node.rect = rect;
                self.display_list_stale = true;
            }
            let child_origin = Point::new(rect.x, rect.y);
            stack.extend(node.children.iter().map(|child| (*child, child_origin)));
        }
        if self.pointer.is_some() {
            self.refresh_hover();
        }
    }

    /// Damages every node whose appearance changed.
    pub(super) fn collect_paint_damage(&mut self) {
        if !std::mem::take(&mut self.paint_dirty) {
            return;
        }
        for node in &mut self.nodes {
            if std::mem::take(&mut node.paint_dirty) {
                self.damage.add(node.rect);
                self.display_list_stale = true;
            }
        }
    }

    pub(super) fn rebuild_display_list(&mut self) {
        if !std::mem::take(&mut self.display_list_stale) {
            return;
        }
        let mut primitives = Vec::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id.index()];
            if node.style.layout.display == Display::None {
                continue;
            }
            let visual = &node.style.visual;
            let has_border = visual.border_width > 0.0 && !visual.border_color.is_transparent();
            if !visual.background.is_transparent() || has_border {
                primitives.push(Primitive::Quad(QuadPrimitive {
                    rect: node.rect,
                    background: visual.background,
                    border_color: visual.border_color,
                    border_width: visual.border_width,
                    corner_radius: visual.corner_radius,
                }));
            }
            if let super::NodeContent::Text(text) = &node.content {
                let padding = node.style.layout.padding;
                let content = cappy_layout::Rect::new(
                    node.rect.x + padding.left,
                    node.rect.y + padding.top,
                    (node.rect.width - padding.horizontal()).max(0.0),
                    (node.rect.height - padding.vertical()).max(0.0),
                );
                let text_style = &node.style.text;
                primitives.push(Primitive::Text(TextPrimitive {
                    rect: content,
                    text: text.clone(),
                    color: text_style.color,
                    font_size: text_style.font_size,
                    line_height: text_style.line_height_px(),
                    weight: text_style.weight.0,
                }));
            }
            stack.extend(node.children.iter().rev());
        }
        self.display_list = Arc::new(primitives);
    }
}

fn text_leaf(text: &str, style: &TextStyle) -> TextLeaf {
    TextLeaf {
        text: text.to_owned(),
        font_size: style.font_size,
        line_height: style.line_height_px(),
        weight: style.weight.0,
    }
}
