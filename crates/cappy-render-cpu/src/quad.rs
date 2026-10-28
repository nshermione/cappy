//! Rounded, bordered rectangles drawn with tiny-skia paths, matching the GPU quad shader: the
//! background fills the whole shape and the border is painted over it as a ring.

use cappy_core::{Color, QuadPrimitive, Rect};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, PixmapMut, Transform};

/// Distance of cubic control points from a corner, as a fraction of the radius, for the closest
/// cubic approximation of a quarter circle.
const CIRCLE_CUBIC_KAPPA: f32 = 0.552_284_8;

/// Paints `quad`, scaled to physical pixels, into `target`, whose top-left pixel sits at `origin`.
pub(crate) fn paint_quad(
    target: &mut PixmapMut<'_>,
    quad: &QuadPrimitive,
    scale_factor: f32,
    origin: (f32, f32),
) {
    let rect = quad.rect.scale(scale_factor);
    if rect.is_empty() {
        return;
    }
    let radius = (quad.corner_radius * scale_factor)
        .min(rect.width / 2.0)
        .min(rect.height / 2.0)
        .max(0.0);
    let transform = Transform::from_translate(-origin.0, -origin.1);

    if !quad.background.is_transparent()
        && let Some(path) = rounded_rect_path(rect, radius)
    {
        fill(target, &path, quad.background, FillRule::Winding, transform);
    }

    let border_width = quad.border_width * scale_factor;
    if border_width > 0.0
        && !quad.border_color.is_transparent()
        && let Some(ring) = border_ring_path(rect, radius, border_width)
    {
        fill(
            target,
            &ring,
            quad.border_color,
            FillRule::EvenOdd,
            transform,
        );
    }
}

fn fill(
    target: &mut PixmapMut<'_>,
    path: &Path,
    color: Color,
    fill_rule: FillRule,
    transform: Transform,
) {
    let [red, green, blue, alpha] = color.to_rgba8();
    let mut paint = Paint::default();
    paint.set_color_rgba8(red, green, blue, alpha);
    paint.anti_alias = true;
    target.fill_path(path, &paint, fill_rule, transform, None);
}

fn rounded_rect_path(rect: Rect, radius: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    push_rounded_rect(&mut builder, rect, radius);
    builder.finish()
}

/// The outer shape minus the inner one; filled with [`FillRule::EvenOdd`]. When the border is
/// wider than the shape, the ring is the whole shape.
fn border_ring_path(rect: Rect, radius: f32, border_width: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    push_rounded_rect(&mut builder, rect, radius);
    let inner = rect.inflate(-border_width);
    if inner.width > 0.0 && inner.height > 0.0 {
        push_rounded_rect(&mut builder, inner, (radius - border_width).max(0.0));
    }
    builder.finish()
}

fn push_rounded_rect(builder: &mut PathBuilder, rect: Rect, radius: f32) {
    let (left, top, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    if radius <= 0.0 {
        if let Some(rect) = tiny_skia::Rect::from_ltrb(left, top, right, bottom) {
            builder.push_rect(rect);
        }
        return;
    }
    let control = radius * (1.0 - CIRCLE_CUBIC_KAPPA);
    builder.move_to(left + radius, top);
    builder.line_to(right - radius, top);
    builder.cubic_to(
        right - control,
        top,
        right,
        top + control,
        right,
        top + radius,
    );
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - control,
        right - control,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(left + radius, bottom);
    builder.cubic_to(
        left + control,
        bottom,
        left,
        bottom - control,
        left,
        bottom - radius,
    );
    builder.line_to(left, top + radius);
    builder.cubic_to(left, top + control, left + control, top, left + radius, top);
    builder.close();
}

#[cfg(test)]
mod tests {
    use tiny_skia::Pixmap;

    use super::*;

    const SIZE: u32 = 40;

    fn render(quad: &QuadPrimitive) -> Pixmap {
        let mut pixmap = Pixmap::new(SIZE, SIZE).expect("non-empty pixmap");
        paint_quad(&mut pixmap.as_mut(), quad, 1.0, (0.0, 0.0));
        pixmap
    }

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let color = pixmap.pixel(x, y).expect("pixel inside the pixmap");
        [color.red(), color.green(), color.blue(), color.alpha()]
    }

    fn bordered(radius: f32) -> QuadPrimitive {
        QuadPrimitive {
            rect: Rect::new(5.0, 5.0, 30.0, 30.0),
            background: Color::rgb(0, 0, 255),
            border_color: Color::rgb(255, 0, 0),
            border_width: 4.0,
            corner_radius: radius,
        }
    }

    #[test]
    fn border_surrounds_the_background() {
        let pixmap = render(&bordered(0.0));
        assert_eq!(pixel(&pixmap, 20, 20), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixmap, 6, 20), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 2, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn rounded_corners_leave_the_corner_pixel_empty() {
        let pixmap = render(&bordered(12.0));
        assert_eq!(pixel(&pixmap, 5, 5), [0, 0, 0, 0]);
        assert_eq!(pixel(&pixmap, 20, 6), [255, 0, 0, 255]);
    }

    #[test]
    fn translucent_background_is_not_tinted_by_the_border() {
        let mut quad = bordered(0.0);
        quad.background = Color::rgba(0, 0, 255, 128);
        let pixmap = render(&quad);
        let [red, _, blue, alpha] = pixel(&pixmap, 20, 20);
        assert_eq!(red, 0);
        assert_eq!((blue, alpha), (128, 128));
    }

    #[test]
    fn origin_offsets_the_drawing() {
        let mut pixmap = Pixmap::new(SIZE, SIZE).expect("non-empty pixmap");
        paint_quad(&mut pixmap.as_mut(), &bordered(0.0), 1.0, (10.0, 10.0));
        assert_eq!(pixel(&pixmap, 10, 10), [0, 0, 255, 255]);
    }
}
