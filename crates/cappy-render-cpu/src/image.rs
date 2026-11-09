//! Images drawn with a tiny-skia pattern, clipped to the element's rounded box.

use cappy_core::{ImagePixels, ImagePrimitive};
use tiny_skia::{
    FillRule, FilterQuality, Mask, Paint, Pattern, Pixmap, PixmapMut, SpreadMode, Transform,
};

use crate::quad::rounded_rect_path;

/// Paints `image`, scaled to physical pixels, into `target`, whose top-left sits at `origin`.
pub(crate) fn paint_image(
    target: &mut PixmapMut<'_>,
    image: &ImagePrimitive,
    scale_factor: f32,
    origin: (f32, f32),
) {
    let placement = image.placement();
    if placement.dest.is_empty() {
        return;
    }
    let Some(raster) = rasterize(&image.pixels) else {
        return;
    };
    let clip = image.rect.scale(scale_factor);
    let dest = placement.dest.scale(scale_factor);
    let radius = (image.corner_radius * scale_factor)
        .min(clip.width / 2.0)
        .min(clip.height / 2.0)
        .max(0.0);
    let transform = Transform::from_translate(-origin.0, -origin.1);
    let (Some(clip_path), Some(dest_path)) = (
        rounded_rect_path(clip, radius),
        rounded_rect_path(dest, 0.0),
    ) else {
        return;
    };
    let Some(mut mask) = Mask::new(target.width(), target.height()) else {
        return;
    };
    mask.fill_path(&clip_path, FillRule::Winding, true, transform);
    if let Some(scroll) = image.clip {
        let scroll = scroll.scale(scale_factor);
        if let Some(path) = rounded_rect_path(scroll, 0.0) {
            mask.intersect_path(&path, FillRule::Winding, true, transform);
        }
    }

    let (scale_x, translate_x) = image_axis(
        dest.x - origin.0,
        dest.width,
        placement.uv_min_x,
        placement.uv_max_x,
        raster.width() as f32,
    );
    let (scale_y, translate_y) = image_axis(
        dest.y - origin.1,
        dest.height,
        placement.uv_min_y,
        placement.uv_max_y,
        raster.height() as f32,
    );
    let paint = Paint {
        shader: Pattern::new(
            raster.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            Transform::from_row(scale_x, 0.0, 0.0, scale_y, translate_x, translate_y),
        ),
        anti_alias: true,
        ..Paint::default()
    };
    target.fill_path(
        &dest_path,
        &paint,
        FillRule::Winding,
        transform,
        Some(&mask),
    );
}

/// Maps a texture axis onto a destination edge in pixmap space.
fn image_axis(
    dest: f32,
    dest_size: f32,
    uv_min: f32,
    uv_max: f32,
    texture_size: f32,
) -> (f32, f32) {
    let span = (uv_max - uv_min) * texture_size;
    if span.abs() <= f32::EPSILON {
        return (1.0, dest);
    }
    let scale = dest_size / span;
    (scale, dest - scale * uv_min * texture_size)
}

fn rasterize(pixels: &ImagePixels) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(pixels.width(), pixels.height())?;
    let (chunks, _) = pixels.rgba().as_chunks::<4>();
    for (pixel, chunk) in pixmap.pixels_mut().iter_mut().zip(chunks) {
        *pixel =
            tiny_skia::ColorU8::from_rgba(chunk[0], chunk[1], chunk[2], chunk[3]).premultiply();
    }
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cappy_core::{ImagePixels, ImagePrimitive, ObjectFit, Rect};
    use tiny_skia::Pixmap;

    use super::*;

    #[test]
    fn contain_centers_a_square_bitmap_in_a_wide_box() {
        let pixels =
            Arc::new(ImagePixels::from_rgba(2, 2, [255, 0, 0, 255].repeat(4)).expect("rgba"));
        let image = ImagePrimitive {
            rect: Rect::new(4.0, 4.0, 32.0, 16.0),
            pixels,
            fit: ObjectFit::Contain,
            corner_radius: 0.0,
            clip: None,
        };
        let mut pixmap = Pixmap::new(40, 24).expect("pixmap");
        paint_image(&mut pixmap.as_mut(), &image, 1.0, (0.0, 0.0));
        let center = pixmap.pixel(20, 12).expect("center");
        assert_eq!(
            [center.red(), center.green(), center.blue(), center.alpha()],
            [255, 0, 0, 255]
        );
        let outside = pixmap.pixel(6, 12).expect("outside the fitted image");
        assert_eq!(outside.alpha(), 0);
    }
}
