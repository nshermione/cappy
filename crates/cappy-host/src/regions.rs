//! Converts logical damage rectangles into whole physical pixel rectangles.

use cappy_core::{Rect, Size};

/// A rectangle of whole physical pixels inside the render target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn to_rect(self) -> Rect {
        Rect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }

    pub fn right(self) -> u32 {
        self.x + self.width
    }

    pub fn bottom(self) -> u32 {
        self.y + self.height
    }
}

/// Scales `damage` to physical pixels, rounds outwards and clips to the target.
pub fn physical_regions(damage: &[Rect], scale_factor: f32, target: (u32, u32)) -> Vec<PixelRect> {
    let bounds = Rect::from_size(Size::new(target.0 as f32, target.1 as f32));
    damage
        .iter()
        .filter_map(|region| region.scale(scale_factor).round_out().intersection(&bounds))
        .filter(|region| !region.is_empty())
        .map(|region| PixelRect {
            x: region.x as u32,
            y: region.y as u32,
            width: region.width as u32,
            height: region.height as u32,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_and_rounds_outwards() {
        let regions = physical_regions(&[Rect::new(10.2, 5.5, 20.1, 4.0)], 2.0, (800, 600));
        assert_eq!(
            regions,
            [PixelRect {
                x: 20,
                y: 11,
                width: 41,
                height: 8,
            }]
        );
    }

    #[test]
    fn clips_to_the_target_and_drops_empty_regions() {
        let damage = [
            Rect::new(-10.0, -10.0, 30.0, 30.0),
            Rect::new(900.0, 0.0, 10.0, 10.0),
        ];
        let regions = physical_regions(&damage, 1.0, (800, 600));
        assert_eq!(
            regions,
            [PixelRect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            }]
        );
    }
}
