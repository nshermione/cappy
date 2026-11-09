//! Paint output: a flat list of primitives plus the damage to repaint.

use std::sync::Arc;

use cappy_layout::{Point, Rect, Size};
use cappy_style::{Background, BoxShadow, Color};

use crate::Damage;
use crate::bitmap::{ImagePixels, ObjectFit};

/// Rotation of a painted box around [`Spin::pivot`]. Zero leaves the box axis-aligned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spin {
    pub pivot: Point,
    pub radians: f32,
}

impl Spin {
    pub const NONE: Self = Self {
        pivot: Point::new(0.0, 0.0),
        radians: 0.0,
    };

    pub fn is_none(self) -> bool {
        self.radians.abs() <= 1.0e-4
    }
}

impl Default for Spin {
    fn default() -> Self {
        Self::NONE
    }
}

pub(crate) fn rect_center(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

/// Scales `rect` around `pivot`. A scale of 1 returns `rect`.
pub fn scale_about(rect: Rect, pivot: Point, scale: f32) -> Rect {
    if (scale - 1.0).abs() <= 1.0e-4 || rect.is_empty() {
        return rect;
    }
    Rect::new(
        pivot.x + (rect.x - pivot.x) * scale,
        pivot.y + (rect.y - pivot.y) * scale,
        (rect.width * scale).max(0.0),
        (rect.height * scale).max(0.0),
    )
}

/// Inverse of a scale-then-rotate around `pivot`. `scale` of 0 is treated as 1.
pub fn unspin_point(point: Point, pivot: Point, scale: f32, radians: f32) -> Point {
    let mut dx = point.x - pivot.x;
    let mut dy = point.y - pivot.y;
    if radians.abs() > 1.0e-4 {
        let (sin, cos) = (-radians).sin_cos();
        let x = cos * dx - sin * dy;
        let y = sin * dx + cos * dy;
        dx = x;
        dy = y;
    }
    let scale = if scale.abs() <= 1.0e-4 { 1.0 } else { scale };
    Point::new(pivot.x + dx / scale, pivot.y + dy / scale)
}

/// Axis-aligned bounds of `rect` after [`Spin`] is applied.
pub fn spun_aabb(rect: Rect, spin: Spin) -> Rect {
    if spin.is_none() || rect.is_empty() {
        return rect;
    }
    let (sin, cos) = spin.radians.sin_cos();
    let corners = [
        Point::new(rect.x, rect.y),
        Point::new(rect.right(), rect.y),
        Point::new(rect.x, rect.bottom()),
        Point::new(rect.right(), rect.bottom()),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for corner in corners {
        let dx = corner.x - spin.pivot.x;
        let dy = corner.y - spin.pivot.y;
        let x = spin.pivot.x + cos * dx - sin * dy;
        let y = spin.pivot.y + sin * dx + cos * dy;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Rect::from_edges(min_x, min_y, max_x, max_y)
}

/// A filled, optionally bordered, optionally rounded rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadPrimitive {
    pub rect: Rect,
    pub background: Background,
    pub border_color: Color,
    pub border_width: f32,
    pub corner_radius: f32,
    /// Ancestor scrollports. `None` means the primitive is not clipped further.
    pub clip: Option<Rect>,
    /// Rotation around [`Spin::pivot`]. [`Spin::NONE`] draws the rect as stored.
    pub spin: Spin,
}

/// A drop shadow drawn behind a box. `rect` is the shadow shape, already offset and spread.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowPrimitive {
    pub rect: Rect,
    pub corner_radius: f32,
    pub blur: f32,
    pub color: Color,
    /// Ancestor scrollports. `None` means the primitive is not clipped further.
    pub clip: Option<Rect>,
    /// Rotation around [`Spin::pivot`], shared with the box that casts the shadow.
    pub spin: Spin,
}

impl ShadowPrimitive {
    /// Pixels the blur can darken, in the same space as `rect`.
    pub fn ink_bounds(&self) -> Rect {
        self.rect.inflate(blur_extent(self.blur))
    }
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
    /// Ancestor scrollports. `None` means the primitive is not clipped further.
    pub clip: Option<Rect>,
}

/// Where an image is drawn inside its box, and which part of the texture that shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImagePlacement {
    pub dest: Rect,
    pub uv_min_x: f32,
    pub uv_min_y: f32,
    pub uv_max_x: f32,
    pub uv_max_y: f32,
}

/// An image scaled into `rect` and clipped to its corner radius.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePrimitive {
    pub rect: Rect,
    pub pixels: Arc<ImagePixels>,
    pub fit: ObjectFit,
    pub corner_radius: f32,
    /// Ancestor scrollports. `None` means the primitive is not clipped further.
    pub clip: Option<Rect>,
    /// Multiplier applied to the texture alpha. 1 leaves the bitmap unchanged.
    pub opacity: f32,
}

impl ImagePrimitive {
    pub fn placement(&self) -> ImagePlacement {
        image_placement(
            self.rect,
            self.pixels.width(),
            self.pixels.height(),
            self.fit,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Shadow(ShadowPrimitive),
    Quad(QuadPrimitive),
    Image(ImagePrimitive),
    Text(TextPrimitive),
}

impl Primitive {
    pub fn bounds(&self) -> Rect {
        let (rect, clip) = match self {
            Self::Shadow(shadow) => (spun_aabb(shadow.ink_bounds(), shadow.spin), shadow.clip),
            Self::Quad(quad) => (spun_aabb(quad.rect, quad.spin), quad.clip),
            Self::Image(image) => (image.rect, image.clip),
            Self::Text(text) => (text.rect, text.clip),
        };
        match clip.and_then(|clip| rect.intersection(&clip)) {
            Some(clipped) => clipped,
            None if clip.is_some() => Rect::ZERO,
            None => rect,
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

/// The border box plus any shadow ink around it.
pub fn paint_bounds(rect: Rect, shadow: BoxShadow) -> Rect {
    shadow_shape(rect, shadow)
        .map(|shape| rect.union(&shape.inflate(shadow.blur_extent())))
        .unwrap_or(rect)
}

/// The shadow shape for `rect`, or `None` when the shadow would not paint.
pub fn shadow_shape(rect: Rect, shadow: BoxShadow) -> Option<Rect> {
    if !shadow.is_visible() {
        return None;
    }
    let shape = Rect::new(
        rect.x + shadow.offset_x - shadow.spread,
        rect.y + shadow.offset_y - shadow.spread,
        (rect.width + shadow.spread * 2.0).max(0.0),
        (rect.height + shadow.spread * 2.0).max(0.0),
    );
    if shape.is_empty() {
        return None;
    }
    Some(shape)
}

/// How far a blur of `blur` pixels is painted past the shadow shape.
pub fn blur_extent(blur: f32) -> f32 {
    BoxShadow {
        blur,
        ..BoxShadow::NONE
    }
    .blur_extent()
}

/// Signed distance to a rounded rectangle. Negative is inside. `local` is relative to the center.
pub fn rounded_box_distance(
    local_x: f32,
    local_y: f32,
    half_width: f32,
    half_height: f32,
    radius: f32,
) -> f32 {
    let radius = radius.min(half_width).min(half_height).max(0.0);
    let corner_x = local_x.abs() - half_width + radius;
    let corner_y = local_y.abs() - half_height + radius;
    let outside = (corner_x.max(0.0).powi(2) + corner_y.max(0.0).powi(2)).sqrt();
    let inside = corner_x.max(corner_y).min(0.0);
    outside + inside - radius
}

/// Scales `image_width` by `image_height` into `rect` according to `fit`.
pub fn image_placement(
    rect: Rect,
    image_width: u32,
    image_height: u32,
    fit: ObjectFit,
) -> ImagePlacement {
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    if image_width <= 0.0 || image_height <= 0.0 || rect.is_empty() {
        return ImagePlacement {
            dest: Rect::ZERO,
            uv_min_x: 0.0,
            uv_min_y: 0.0,
            uv_max_x: 0.0,
            uv_max_y: 0.0,
        };
    }
    match fit {
        ObjectFit::Fill => ImagePlacement {
            dest: rect,
            uv_min_x: 0.0,
            uv_min_y: 0.0,
            uv_max_x: 1.0,
            uv_max_y: 1.0,
        },
        ObjectFit::Contain => {
            let scale = (rect.width / image_width).min(rect.height / image_height);
            let width = image_width * scale;
            let height = image_height * scale;
            ImagePlacement {
                dest: Rect::new(
                    rect.x + (rect.width - width) * 0.5,
                    rect.y + (rect.height - height) * 0.5,
                    width,
                    height,
                ),
                uv_min_x: 0.0,
                uv_min_y: 0.0,
                uv_max_x: 1.0,
                uv_max_y: 1.0,
            }
        }
        ObjectFit::Cover => {
            let scale = (rect.width / image_width).max(rect.height / image_height);
            let drawn_width = image_width * scale;
            let drawn_height = image_height * scale;
            let crop_x = ((drawn_width - rect.width) * 0.5 / drawn_width).clamp(0.0, 0.5);
            let crop_y = ((drawn_height - rect.height) * 0.5 / drawn_height).clamp(0.0, 0.5);
            ImagePlacement {
                dest: rect,
                uv_min_x: crop_x,
                uv_min_y: crop_y,
                uv_max_x: 1.0 - crop_x,
                uv_max_y: 1.0 - crop_y,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_shadow_grows_only_on_that_side() {
        let shadow = BoxShadow {
            offset_x: 10.0,
            offset_y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: Color::BLACK,
        };
        let bounds = paint_bounds(Rect::new(0.0, 0.0, 40.0, 40.0), shadow);
        assert_eq!(bounds, Rect::new(0.0, 0.0, 50.0, 40.0));
    }

    #[test]
    fn contain_letterboxes_and_cover_crops() {
        let rect = Rect::new(0.0, 0.0, 40.0, 20.0);
        let contained = image_placement(rect, 20, 20, ObjectFit::Contain);
        assert_eq!(contained.dest, Rect::new(10.0, 0.0, 20.0, 20.0));
        let covered = image_placement(rect, 20, 20, ObjectFit::Cover);
        assert_eq!(covered.dest, rect);
        assert!(covered.uv_min_y > 0.0 && covered.uv_max_y < 1.0);
        assert_eq!(covered.uv_min_x, 0.0);
    }
}
