//! Paint output: a flat list of primitives plus the damage to repaint.

use std::sync::Arc;

use cappy_layout::{Rect, Size};
use cappy_style::Color;

use crate::Damage;

/// A filled, optionally bordered, optionally rounded rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadPrimitive {
    pub rect: Rect,
    pub background: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub corner_radius: f32,
}

/// A block of text laid out inside `rect`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitive {
    pub rect: Rect,
    pub text: Arc<str>,
    pub color: Color,
    pub font_size: f32,
    /// Line height in logical pixels.
    pub line_height: f32,
    pub weight: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Quad(QuadPrimitive),
    Text(TextPrimitive),
}

impl Primitive {
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Quad(quad) => quad.rect,
            Self::Text(text) => text.rect,
        }
    }
}

/// Everything a renderer needs for one frame. `Send + Sync`, so it can cross into a render thread.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderFrame {
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f32,
    /// Primitives in paint order (back to front), in logical pixels.
    pub primitives: Arc<Vec<Primitive>>,
    /// Regions changed since the previous frame, in logical pixels.
    pub damage: Damage,
}

impl RenderFrame {
    pub fn logical_size(&self) -> Size {
        Size::new(
            self.physical_width as f32 / self.scale_factor,
            self.physical_height as f32 / self.scale_factor,
        )
    }
}
