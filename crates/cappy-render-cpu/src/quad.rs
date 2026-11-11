//! Rounded, bordered rectangles drawn with tiny-skia paths, matching the GPU quad shader: the
//! background fills the whole shape and the border is painted over it as a ring.

use cappy_core::rounded_box_distance;
use cappy_core::rounded_box_distance_corners;
use cappy_core::{
    Background, BoxShadow, Color, Corners, Edges, Point, QuadPrimitive, Rect, ShadowPrimitive,
    Spin, spun_aabb, unspin_point,
};
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
    if !(quad.border_width.is_uniform() && quad.corner_radius.is_uniform()) {
        paint_edged_quad(target, quad, scale_factor, origin);
        return;
    }
    let rect = quad.rect.scale(scale_factor);
    if rect.is_empty() {
        return;
    }
    let radius = (quad.corner_radius.top_left * scale_factor)
        .min(rect.width / 2.0)
        .min(rect.height / 2.0)
        .max(0.0);
    let mut transform = Transform::from_translate(-origin.0, -origin.1);
    if !quad.spin.is_none() {
        transform = transform.pre_concat(Transform::from_rotate_at(
            quad.spin.radians.to_degrees(),
            quad.spin.pivot.x * scale_factor,
            quad.spin.pivot.y * scale_factor,
        ));
    }
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
            mask.as_ref(),
        );
    }

    let border_width = quad.border_width.top * scale_factor;
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
    let uniform_radius = shadow.corner_radius.is_uniform();
    let radius = (shadow.corner_radius.top_left * scale_factor)
        .min(shape.width / 2.0)
        .min(shape.height / 2.0)
        .max(0.0);
    let corner_radii = shadow.corner_radius.scaled(scale_factor);
    let extent = BoxShadow {
        blur,
        ..BoxShadow::NONE
    }
    .blur_extent();
    let spin = Spin {
        pivot: Point::new(
            shadow.spin.pivot.x * scale_factor,
            shadow.spin.pivot.y * scale_factor,
        ),
        radians: shadow.spin.radians,
    };
    let ink = spun_aabb(shape.inflate(extent + 1.0), spin);
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
            let (sample_x, sample_y) = if spin.is_none() {
                (world_x, world_y)
            } else {
                let local =
                    unspin_point(Point::new(world_x, world_y), spin.pivot, 1.0, spin.radians);
                (local.x, local.y)
            };
            let distance = if uniform_radius {
                rounded_box_distance(
                    sample_x - center_x,
                    sample_y - center_y,
                    half_width,
                    half_height,
                    radius,
                )
            } else {
                rounded_box_distance_corners(
                    sample_x - center_x,
                    sample_y - center_y,
                    half_width,
                    half_height,
                    corner_radii,
                )
            };
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
    if clip_covers_target(clip, origin, target.width(), target.height()) {
        return None;
    }
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

/// The scratch pixmap is already inside `clip`, so a mask the size of that pixmap would only
/// repeat the same rectangle.
fn clip_covers_target(clip: Rect, origin: (f32, f32), width: u32, height: u32) -> bool {
    clip.x <= origin.0
        && clip.y <= origin.1
        && clip.right() >= origin.0 + width as f32
        && clip.bottom() >= origin.1 + height as f32
}

fn fill_background(
    target: &mut PixmapMut<'_>,
    path: &Path,
    background: Background,
    rect: Rect,
    transform: Transform,
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
            // Leave the stops in the same space as `path`. `fill_path` applies `transform` to
            // the shader as well as the path. Mapping the points here shifts the gradient a
            // second time, so a scratch that does not start at (0, 0) paints one solid stop.
            let start = tiny_skia::Point::from_xy(
                rect.x + start.x * rect.width,
                rect.y + start.y * rect.height,
            );
            let end = tiny_skia::Point::from_xy(
                rect.x + end.x * rect.width,
                rect.y + end.y * rect.height,
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

/// A box whose edges or corners differ. Uniform boxes stay on [`paint_quad`]'s original path.
fn paint_edged_quad(
    target: &mut PixmapMut<'_>,
    quad: &QuadPrimitive,
    scale_factor: f32,
    origin: (f32, f32),
) {
    let rect = quad.rect.scale(scale_factor);
    if rect.is_empty() {
        return;
    }
    let borders = quad.border_width.scaled(scale_factor);
    let radii = clamp_corners(
        quad.corner_radius.scaled(scale_factor),
        rect.width,
        rect.height,
    );
    let mut transform = Transform::from_translate(-origin.0, -origin.1);
    if !quad.spin.is_none() {
        transform = transform.pre_concat(Transform::from_rotate_at(
            quad.spin.radians.to_degrees(),
            quad.spin.pivot.x * scale_factor,
            quad.spin.pivot.y * scale_factor,
        ));
    }
    let mask = quad
        .clip
        .and_then(|clip| clip_mask(target, clip, scale_factor, origin));
    if !quad.background.is_transparent()
        && let Some(path) = corner_path(rect, radii)
    {
        fill_background(
            target,
            &path,
            quad.background,
            rect,
            transform,
            mask.as_ref(),
        );
    }
    if borders.any_positive()
        && !quad.border_color.is_transparent()
        && let Some(ring) = edge_ring(rect, radii, borders)
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

fn clamp_corners(corners: Corners, width: f32, height: f32) -> Corners {
    let limit = (width * 0.5).min(height * 0.5).max(0.0);
    Corners {
        top_left: corners.top_left.clamp(0.0, limit),
        top_right: corners.top_right.clamp(0.0, limit),
        bottom_right: corners.bottom_right.clamp(0.0, limit),
        bottom_left: corners.bottom_left.clamp(0.0, limit),
    }
}

fn corner_path(rect: Rect, corners: Corners) -> Option<Path> {
    let mut builder = PathBuilder::new();
    push_corner_rect(&mut builder, rect, corners);
    builder.finish()
}

fn edge_ring(rect: Rect, radii: Corners, border: Edges) -> Option<Path> {
    let border = Edges {
        top: border.top.max(0.0),
        right: border.right.max(0.0),
        bottom: border.bottom.max(0.0),
        left: border.left.max(0.0),
    };
    let mut builder = PathBuilder::new();
    push_corner_rect(&mut builder, rect, radii);
    let inner = Rect::new(
        rect.x + border.left,
        rect.y + border.top,
        (rect.width - border.horizontal()).max(0.0),
        (rect.height - border.vertical()).max(0.0),
    );
    if inner.width > 0.0 && inner.height > 0.0 {
        let inner_radii = clamp_corners(
            Corners {
                top_left: (radii.top_left - border.top.max(border.left)).max(0.0),
                top_right: (radii.top_right - border.top.max(border.right)).max(0.0),
                bottom_right: (radii.bottom_right - border.bottom.max(border.right)).max(0.0),
                bottom_left: (radii.bottom_left - border.bottom.max(border.left)).max(0.0),
            },
            inner.width,
            inner.height,
        );
        push_corner_rect(&mut builder, inner, inner_radii);
    }
    builder.finish()
}

fn push_corner_rect(builder: &mut PathBuilder, rect: Rect, corners: Corners) {
    let (left, top, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    let (tl, tr, br, bl) = (
        corners.top_left,
        corners.top_right,
        corners.bottom_right,
        corners.bottom_left,
    );
    if tl <= 0.0 && tr <= 0.0 && br <= 0.0 && bl <= 0.0 {
        if let Some(rect) = tiny_skia::Rect::from_ltrb(left, top, right, bottom) {
            builder.push_rect(rect);
        }
        return;
    }
    let control = |radius: f32| radius * (1.0 - CIRCLE_CUBIC_KAPPA);
    builder.move_to(left + tl, top);
    builder.line_to(right - tr, top);
    if tr > 0.0 {
        let curve = control(tr);
        builder.cubic_to(right - curve, top, right, top + curve, right, top + tr);
    }
    builder.line_to(right, bottom - br);
    if br > 0.0 {
        let curve = control(br);
        builder.cubic_to(
            right,
            bottom - curve,
            right - curve,
            bottom,
            right - br,
            bottom,
        );
    }
    builder.line_to(left + bl, bottom);
    if bl > 0.0 {
        let curve = control(bl);
        builder.cubic_to(
            left + curve,
            bottom,
            left,
            bottom - curve,
            left,
            bottom - bl,
        );
    }
    builder.line_to(left, top + tl);
    if tl > 0.0 {
        let curve = control(tl);
        builder.cubic_to(left, top + curve, left + curve, top, left + tl, top);
    }
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
            border_width: Edges::all(4.0),
            corner_radius: Corners::all(radius),
            clip: None,
            spin: Spin::NONE,
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
    fn clip_covering_the_target_matches_an_unclipped_quad() {
        let open = render(&bordered(0.0));
        let mut covered = bordered(0.0);
        covered.clip = Some(Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32));
        let covered = render(&covered);
        assert_eq!(open.data(), covered.data());
    }

    #[test]
    fn clip_still_cuts_pixels_outside_it() {
        let mut quad = bordered(0.0);
        quad.clip = Some(Rect::new(0.0, 0.0, 12.0, SIZE as f32));
        let pixmap = render(&quad);
        assert_ne!(pixel(&pixmap, 8, 20), [0, 0, 0, 0]);
        assert_eq!(pixel(&pixmap, 20, 20), [0, 0, 0, 0]);
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
        quad.border_width = Edges::ZERO;
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
    fn a_gradient_in_an_offset_scratch_keeps_both_stops() {
        use cappy_core::UnitPoint;

        let mut quad = bordered(0.0);
        quad.border_width = Edges::ZERO;
        quad.rect = Rect::new(40.0, 20.0, 30.0, 10.0);
        quad.background = Background::Linear {
            from: Color::rgb(255, 0, 0),
            to: Color::rgb(0, 0, 255),
            start: UnitPoint::new(0.0, 0.5),
            end: UnitPoint::new(1.0, 0.5),
        };
        let mut scratch = Pixmap::new(30, 10).expect("scratch");
        paint_quad(&mut scratch.as_mut(), &quad, 1.0, (40.0, 20.0));
        let left = pixel(&scratch, 2, 5);
        let right = pixel(&scratch, 27, 5);
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
                corner_radius: Corners::ZERO,
                blur: 0.0,
                color: Color::rgba(0, 0, 0, 255),
                clip: None,
                spin: Spin::NONE,
            },
            1.0,
            (0.0, 0.0),
        );
        assert_eq!(pixel(&pixmap, 12, 12), [0, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 2, 12), [0, 0, 0, 0]);
    }

    #[test]
    fn a_quarter_turn_swings_a_wide_bar_upright() {
        let mut quad = bordered(0.0);
        quad.border_width = Edges::ZERO;
        quad.rect = Rect::new(8.0, 16.0, 24.0, 8.0);
        quad.spin = Spin {
            pivot: Point::new(20.0, 20.0),
            radians: std::f32::consts::FRAC_PI_2,
        };
        let pixmap = render(&quad);
        assert_eq!(pixel(&pixmap, 20, 30), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixmap, 10, 20), [0, 0, 0, 0]);
    }

    #[test]
    fn a_top_border_does_not_paint_the_left_edge() {
        let mut quad = bordered(0.0);
        quad.border_width = Edges {
            top: 4.0,
            ..Edges::ZERO
        };
        let pixmap = render(&quad);
        assert_eq!(pixel(&pixmap, 20, 7), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 7, 20), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixmap, 20, 20), [0, 0, 255, 255]);
    }

    #[test]
    fn only_the_top_left_corner_is_rounded() {
        let mut quad = bordered(0.0);
        quad.border_width = Edges::ZERO;
        quad.corner_radius = Corners {
            top_left: 12.0,
            ..Corners::ZERO
        };
        let pixmap = render(&quad);
        assert_eq!(pixel(&pixmap, 5, 5), [0, 0, 0, 0]);
        assert_eq!(pixel(&pixmap, 33, 6), [0, 0, 255, 255]);
    }
}
