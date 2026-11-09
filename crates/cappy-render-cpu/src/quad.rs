//! Rounded, bordered rectangles drawn with tiny-skia paths, matching the GPU quad shader: the
//! background fills the whole shape and the border is painted over it as a ring.

use cappy_core::rounded_box_distance;
use cappy_core::{Background, BoxShadow, Color, QuadPrimitive, Rect, ShadowPrimitive};
use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, PixmapMut, SpreadMode,
    Transform,
};

use crate::blend::{blend_into, premultiply, scale};

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
    let mask = quad
        .clip
        .and_then(|clip| clip_mask(target, clip, scale_factor, origin));

    if !quad.background.is_transparent()
        && let Some(path) = rounded_rect_path(rect, radius)
    {
        fill_background(
            target,
            &path,
            quad.background,
            rect,
            transform,
            origin,
            mask.as_ref(),
        );
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
            mask.as_ref(),
        );
    }
}

/// Paints `shadow`, scaled to physical pixels, into `target`, whose top-left sits at `origin`.
pub(crate) fn paint_shadow(
    target: &mut PixmapMut<'_>,
    shadow: &ShadowPrimitive,
    scale_factor: f32,
    origin: (f32, f32),
) {
    let shape = shadow.rect.scale(scale_factor);
    if shape.is_empty() {
        return;
    }
    let blur = shadow.blur * scale_factor;
    let radius = (shadow.corner_radius * scale_factor)
        .min(shape.width / 2.0)
        .min(shape.height / 2.0)
        .max(0.0);
    let extent = BoxShadow {
        blur,
        ..BoxShadow::NONE
    }
    .blur_extent();
    let ink = shape.inflate(extent + 1.0);
    let left = (ink.x - origin.0).floor().max(0.0) as i32;
    let top = (ink.y - origin.1).floor().max(0.0) as i32;
    let right = ((ink.right() - origin.0).ceil() as i32).min(target.width() as i32);
    let bottom = ((ink.bottom() - origin.1).ceil() as i32).min(target.height() as i32);
    if left >= right || top >= bottom {
        return;
    }
    let color = premultiply(shadow.color.to_rgba8());
    let half_width = shape.width * 0.5;
    let half_height = shape.height * 0.5;
    let center_x = shape.x + half_width;
    let center_y = shape.y + half_height;
    let width = target.width() as usize;
    let pixels = target.data_mut();
    for y in top..bottom {
        for x in left..right {
            let world_x = x as f32 + 0.5 + origin.0;
            let world_y = y as f32 + 0.5 + origin.1;
            if let Some(clip) = shadow.clip {
                let clip = clip.scale(scale_factor);
                if world_x < clip.x
                    || world_y < clip.y
                    || world_x >= clip.right()
                    || world_y >= clip.bottom()
                {
                    continue;
                }
            }
            let distance = rounded_box_distance(
                world_x - center_x,
                world_y - center_y,
                half_width,
                half_height,
                radius,
            );
            let coverage = BoxShadow::coverage(distance, blur);
            if coverage <= 0.0 {
                continue;
            }
            let coverage = (coverage * 255.0).round().clamp(0.0, 255.0) as u8;
            let offset = (y as usize * width + x as usize) * 4;
            blend_into(&mut pixels[offset..offset + 4], scale(color, coverage));
        }
    }
}

pub(crate) fn rounded_rect_path(rect: Rect, radius: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    push_rounded_rect(&mut builder, rect, radius);
    builder.finish()
}

fn clip_mask(
    target: &PixmapMut<'_>,
    clip: Rect,
    scale_factor: f32,
    origin: (f32, f32),
) -> Option<Mask> {
    let clip = clip.scale(scale_factor);
    let mut mask = Mask::new(target.width(), target.height())?;
    let local = Rect::new(
        clip.x - origin.0,
        clip.y - origin.1,
        clip.width,
        clip.height,
    );
    let path = rounded_rect_path(local, 0.0)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

fn fill_background(
    target: &mut PixmapMut<'_>,
    path: &Path,
    background: Background,
    rect: Rect,
    transform: Transform,
    origin: (f32, f32),
    mask: Option<&Mask>,
) {
    match background {
        Background::Solid(color) => fill(target, path, color, FillRule::Winding, transform, mask),
        Background::Linear {
            from,
            to,
            start,
            end,
        } => {
            let start = tiny_skia::Point::from_xy(
                rect.x + start.x * rect.width - origin.0,
                rect.y + start.y * rect.height - origin.1,
            );
            let end = tiny_skia::Point::from_xy(
                rect.x + end.x * rect.width - origin.0,
                rect.y + end.y * rect.height - origin.1,
            );
            let stops = vec![
                GradientStop::new(0.0, skia_color(from)),
                GradientStop::new(1.0, skia_color(to)),
            ];
            let Some(shader) =
                LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity())
            else {
                fill(target, path, from, FillRule::Winding, transform, mask);
                return;
            };
            let paint = Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            };
            target.fill_path(path, &paint, FillRule::Winding, transform, mask);
        }
    }
}

fn skia_color(color: Color) -> tiny_skia::Color {
    let [red, green, blue, alpha] = color.to_rgba8();
    tiny_skia::Color::from_rgba8(red, green, blue, alpha)
}

fn fill(
    target: &mut PixmapMut<'_>,
    path: &Path,
    color: Color,
    fill_rule: FillRule,
    transform: Transform,
    mask: Option<&Mask>,
) {
    let [red, green, blue, alpha] = color.to_rgba8();
    let mut paint = Paint::default();
    paint.set_color_rgba8(red, green, blue, alpha);
    paint.anti_alias = true;
    target.fill_path(path, &paint, fill_rule, transform, mask);
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
            background: Background::Solid(Color::rgb(0, 0, 255)),
            border_color: Color::rgb(255, 0, 0),
            border_width: 4.0,
            corner_radius: radius,
            clip: None,
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
        quad.background = Background::Solid(Color::rgba(0, 0, 255, 128));
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

    #[test]
    fn a_horizontal_gradient_changes_across_the_box() {
        use cappy_core::UnitPoint;

        let mut quad = bordered(0.0);
        quad.border_width = 0.0;
        quad.background = Background::Linear {
            from: Color::rgb(255, 0, 0),
            to: Color::rgb(0, 0, 255),
            start: UnitPoint::new(0.0, 0.5),
            end: UnitPoint::new(1.0, 0.5),
        };
        let pixmap = render(&quad);
        let left = pixel(&pixmap, 8, 20);
        let right = pixel(&pixmap, 32, 20);
        assert!(left[0] > left[2], "left side is red: {left:?}");
        assert!(right[2] > right[0], "right side is blue: {right:?}");
    }

    #[test]
    fn a_hard_shadow_paints_outside_the_box() {
        use cappy_core::ShadowPrimitive;

        let mut pixmap = Pixmap::new(SIZE, SIZE).expect("non-empty pixmap");
        paint_shadow(
            &mut pixmap.as_mut(),
            &ShadowPrimitive {
                rect: Rect::new(8.0, 8.0, 16.0, 16.0),
                corner_radius: 0.0,
                blur: 0.0,
                color: Color::rgba(0, 0, 0, 255),
                clip: None,
            },
            1.0,
            (0.0, 0.0),
        );
        assert_eq!(pixel(&pixmap, 12, 12), [0, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 2, 12), [0, 0, 0, 0]);
    }
}
