//! The style, layout and paint passes run by [`Ui::update`].

use std::sync::Arc;

use cappy_layout::{Point, Rect, TextLeaf};
use cappy_style::{Background, Color, Display, Extent, LayoutStyle, TextStyle, VisualStyle};

use super::scroll::{self, axis_clip, horizontal_thumb, nest_clip, scrollport, vertical_thumb};
use super::{NodeId, Ui};
use crate::paint::{
    ImagePrimitive, Primitive, QuadPrimitive, ShadowPrimitive, TextPrimitive, paint_bounds,
    shadow_shape,
};

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
        if self.nodes[node.index()].suppressed {
            style.display = Display::None;
        }
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
        let resolved = self.theme.get().resolve(
            parent_text,
            &node.all_classes(),
            &node.effective_inline_style(),
            node.interaction,
            node.stylesheet.as_deref(),
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
    ///
    /// A scroll change sets [`Ui::positions_dirty`] and skips taffy: child positions are the
    /// stored layout rects shifted by the current offset.
    pub(super) fn run_layout(&mut self) {
        if std::mem::take(&mut self.layout_dirty) {
            let viewport = self.logical_viewport();
            self.layout
                .compute(self.root, viewport, self.measurer.as_mut())
                .expect(LAYOUT_OUT_OF_SYNC);
            self.clamp_scroll_targets();
            self.positions_dirty = true;
        }
        if !std::mem::take(&mut self.positions_dirty) {
            return;
        }
        self.place_nodes();
        if self.pointer.is_some() {
            self.refresh_hover();
        }
    }

    /// Moves each scrolling offset toward its target. A change only repositions; taffy stays.
    pub(super) fn step_scroll(&mut self, dt: f32) {
        let mut moved = false;
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            if !node.alive {
                continue;
            }
            let next = scroll::ease_point(node.scroll, node.scroll_target, dt);
            if next == node.scroll {
                continue;
            }
            let id = NodeId::new(index as u32, node.generation);
            self.nodes[index].scroll = next;
            self.damage_scrollport(id);
            moved = true;
        }
        if moved {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
    }

    fn clamp_scroll_targets(&mut self) {
        for index in 0..self.nodes.len() {
            if !self.nodes[index].alive {
                continue;
            }
            let id = NodeId::new(index as u32, self.nodes[index].generation);
            let max = self.scroll_max(id);
            let scrolls_x = self.nodes[index].style.layout.overflow_x.scrolls();
            let scrolls_y = self.nodes[index].style.layout.overflow_y.scrolls();
            let node = &mut self.nodes[index];
            let mut target = node.scroll_target;
            let mut scroll = node.scroll;
            if scrolls_x {
                target.x = target.x.clamp(0.0, max.x);
            } else {
                target.x = 0.0;
                scroll.x = 0.0;
            }
            if scrolls_y {
                target.y = target.y.clamp(0.0, max.y);
            } else {
                target.y = 0.0;
                scroll.y = 0.0;
            }
            if scroll != node.scroll || target != node.scroll_target {
                node.scroll = scroll;
                node.scroll_target = target;
            }
        }
    }

    /// Puts `offset` on one axis and skips easing, so a scrollbar drag tracks the pointer.
    pub(super) fn jump_scroll(&mut self, id: NodeId, vertical: bool, offset: f32) {
        if !self.is_current(id) {
            return;
        }
        let max = self.scroll_max(id);
        let node = &mut self.nodes[id.index()];
        let mut next = node.scroll;
        if vertical {
            next.y = offset.clamp(0.0, max.y.max(0.0));
        } else {
            next.x = offset.clamp(0.0, max.x.max(0.0));
        }
        if next == node.scroll && next == node.scroll_target {
            return;
        }
        node.scroll = next;
        node.scroll_target = next;
        self.damage_scrollport(id);
        self.positions_dirty = true;
        self.display_list_stale = true;
    }

    pub(super) fn scroll_max(&self, id: NodeId) -> Point {
        let overflow = self
            .layout
            .scrollable_overflow(id)
            .expect(LAYOUT_OUT_OF_SYNC);
        let mut max = overflow.max_offset();
        let layout = &self.nodes[id.index()].style.layout;
        if !layout.overflow_x.scrolls() {
            max.x = 0.0;
        }
        if !layout.overflow_y.scrolls() {
            max.y = 0.0;
        }
        max
    }

    fn place_nodes(&mut self) {
        let mut stack = vec![Placed {
            id: self.root,
            origin: Point::default(),
            clip: None,
        }];
        while let Some(Placed { id, origin, clip }) = stack.pop() {
            let rect = self
                .layout
                .local_rect(id)
                .expect(LAYOUT_OUT_OF_SYNC)
                .translate(origin);
            let (previous, shadow, moved) = {
                let node = &mut self.nodes[id.index()];
                let moved = node.rect != rect;
                let previous = node.painted;
                let shadow = node.style.visual.shadow;
                if moved {
                    node.rect = rect;
                }
                (previous, shadow, moved)
            };
            if moved {
                self.damage_clipped(previous, clip);
                self.damage_clipped(paint_bounds(rect, shadow), clip);
                self.display_list_stale = true;
            }
            let node = &self.nodes[id.index()];
            let child_origin = Point::new(rect.x - node.scroll.x, rect.y - node.scroll.y);
            let child_clip = if node.style.layout.clips() {
                nest_clip(
                    clip,
                    axis_clip(
                        scrollport(rect, node.style.visual.border_width),
                        node.style.layout.overflow_x.clips(),
                        node.style.layout.overflow_y.clips(),
                    ),
                )
            } else {
                clip
            };
            stack.extend(node.children.iter().rev().map(|child| Placed {
                id: *child,
                origin: child_origin,
                clip: child_clip,
            }));
        }
    }

    fn damage_scrollport(&mut self, id: NodeId) {
        let node = &self.nodes[id.index()];
        self.damage
            .add(scrollport(node.rect, node.style.visual.border_width));
    }

    fn damage_clipped(&mut self, rect: Rect, clip: Option<Rect>) {
        let Some(rect) = clip_rect(rect, clip) else {
            return;
        };
        self.damage.add(rect);
    }

    /// Damages every node whose appearance changed.
    pub(super) fn collect_paint_damage(&mut self) {
        if !std::mem::take(&mut self.paint_dirty) {
            return;
        }
        for node in &mut self.nodes {
            if std::mem::take(&mut node.paint_dirty) {
                self.damage.add(node.painted);
                self.damage
                    .add(paint_bounds(node.rect, node.style.visual.shadow));
                self.display_list_stale = true;
            }
        }
    }

    pub(super) fn rebuild_display_list(&mut self) {
        if !std::mem::take(&mut self.display_list_stale) {
            return;
        }
        let mut primitives = Vec::new();
        let mut painted = vec![Rect::ZERO; self.nodes.len()];
        let mut stack = vec![Painted::Enter {
            id: self.root,
            clip: None,
        }];
        while let Some(step) = stack.pop() {
            match step {
                Painted::Thumb { id, clip } => {
                    self.push_thumbs(&mut primitives, id, clip);
                    continue;
                }
                Painted::Enter { id, clip } => {
                    let node = &self.nodes[id.index()];
                    if !node.alive || node.suppressed || node.style.layout.display == Display::None
                    {
                        continue;
                    }
                    let visual = &node.style.visual;
                    painted[id.index()] = paint_bounds(node.rect, visual.shadow);
                    push_box(&mut primitives, node.rect, visual, clip);
                    match &node.content {
                        super::NodeContent::Text(text) => {
                            let padding = node.style.layout.padding;
                            let content = Rect::new(
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
                                clip,
                            }));
                        }
                        super::NodeContent::Image { pixels, fit } => {
                            primitives.push(Primitive::Image(ImagePrimitive {
                                rect: node.rect,
                                pixels: pixels.clone(),
                                fit: *fit,
                                corner_radius: visual.corner_radius,
                                clip,
                            }));
                        }
                        super::NodeContent::Container => {}
                    }
                    let child_clip = if node.style.layout.clips() {
                        nest_clip(
                            clip,
                            axis_clip(
                                scrollport(node.rect, visual.border_width),
                                node.style.layout.overflow_x.clips(),
                                node.style.layout.overflow_y.clips(),
                            ),
                        )
                    } else {
                        clip
                    };
                    stack.push(Painted::Thumb {
                        id,
                        clip: child_clip,
                    });
                    stack.extend(node.children.iter().rev().map(|child| Painted::Enter {
                        id: *child,
                        clip: child_clip,
                    }));
                }
            }
        }
        for (node, ink) in self.nodes.iter_mut().zip(painted) {
            node.painted = ink;
        }
        self.display_list = Arc::new(primitives);
    }

    fn push_thumbs(&self, primitives: &mut Vec<Primitive>, id: NodeId, clip: Option<Rect>) {
        let node = &self.nodes[id.index()];
        let layout = &node.style.layout;
        if !layout.overflow_x.scrolls() && !layout.overflow_y.scrolls() {
            return;
        }
        let port = scrollport(node.rect, node.style.visual.border_width);
        let max = self.scroll_max(id);
        if layout.overflow_y.scrolls()
            && let Some(rect) = vertical_thumb(port, node.scroll.y, max.y)
        {
            push_thumb(primitives, rect, clip);
        }
        if layout.overflow_x.scrolls()
            && let Some(rect) = horizontal_thumb(port, node.scroll.x, max.x)
        {
            push_thumb(primitives, rect, clip);
        }
    }
}

struct Placed {
    id: NodeId,
    origin: Point,
    clip: Option<Rect>,
}

enum Painted {
    Enter {
        id: NodeId,
        clip: Option<Rect>,
    },
    /// Drawn after the node's descendants so the thumb stays above the content.
    Thumb {
        id: NodeId,
        clip: Option<Rect>,
    },
}

fn clip_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersection(&clip),
        None => Some(rect),
    }
}

fn push_box(primitives: &mut Vec<Primitive>, rect: Rect, visual: &VisualStyle, clip: Option<Rect>) {
    if let Some(shape) = shadow_shape(rect, visual.shadow) {
        primitives.push(Primitive::Shadow(ShadowPrimitive {
            rect: shape,
            corner_radius: visual.corner_radius,
            blur: visual.shadow.blur.max(0.0),
            color: visual.shadow.color,
            clip,
        }));
    }
    let has_border = visual.border_width > 0.0 && !visual.border_color.is_transparent();
    if !visual.background.is_transparent() || has_border {
        primitives.push(Primitive::Quad(QuadPrimitive {
            rect,
            background: visual.background,
            border_color: visual.border_color,
            border_width: visual.border_width,
            corner_radius: visual.corner_radius,
            clip,
        }));
    }
}

fn push_thumb(primitives: &mut Vec<Primitive>, rect: Rect, clip: Option<Rect>) {
    primitives.push(Primitive::Quad(QuadPrimitive {
        rect,
        background: Background::Solid(scroll::THUMB_COLOR),
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        corner_radius: 2.0,
        clip,
    }));
}

fn text_leaf(text: &str, style: &TextStyle) -> TextLeaf {
    TextLeaf {
        text: text.to_owned(),
        font_size: style.font_size,
        line_height: style.line_height_px(),
        weight: style.weight.0,
    }
}
