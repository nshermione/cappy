//! The debug overlay outlining repainted regions, drawn onto the presented `0RGB` buffer.

use cappy_core::Color;
use cappy_host::PixelRect;

use crate::blend::{from_xrgb, over, premultiply, to_xrgb};

const DAMAGE_OVERLAY_FILL: Color = Color::rgba(255, 40, 80, 40);
const DAMAGE_OVERLAY_BORDER: Color = Color::rgba(255, 40, 80, 220);
const DAMAGE_OVERLAY_BORDER_WIDTH: u32 = 2;

/// Tints every rectangle and outlines its edges. `buffer` holds `width` pixels per row.
pub(crate) fn draw_damage_overlay(buffer: &mut [u32], width: u32, rects: &[PixelRect]) {
    let fill = premultiply(DAMAGE_OVERLAY_FILL.to_rgba8());
    let border = premultiply(DAMAGE_OVERLAY_BORDER.to_rgba8());
    let height = buffer.len() as u32 / width.max(1);
    for rect in rects {
        let right = rect.right().min(width);
        let bottom = rect.bottom().min(height);
        for y in rect.y..bottom {
            for x in rect.x..right {
                let on_border = x < rect.x + DAMAGE_OVERLAY_BORDER_WIDTH
                    || y < rect.y + DAMAGE_OVERLAY_BORDER_WIDTH
                    || x + DAMAGE_OVERLAY_BORDER_WIDTH >= right
                    || y + DAMAGE_OVERLAY_BORDER_WIDTH >= bottom;
                let source = if on_border { border } else { fill };
                let pixel = &mut buffer[(y * width + x) as usize];
                *pixel = to_xrgb(over(source, from_xrgb(*pixel)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines_and_tints_the_region() {
        let width = 10;
        let mut buffer = vec![0u32; 100];
        let region = PixelRect {
            x: 2,
            y: 2,
            width: 6,
            height: 6,
        };
        draw_damage_overlay(&mut buffer, width, &[region]);
        let at = |x: u32, y: u32| buffer[(y * width + x) as usize];
        assert_eq!(at(0, 0), 0, "outside the region");
        assert!(at(2, 2) >> 16 > 200, "border is strongly red");
        let inside = at(5, 5) >> 16;
        assert!(inside > 0 && inside < 100, "inside is lightly tinted");
    }
}
