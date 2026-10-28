//! Converts logical damage rectangles into physical scissor rectangles.

use cappy_core::{Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ScissorRect {
    pub fn to_rect(self) -> Rect {
        Rect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }
}

/// Scales `damage` to physical pixels, rounds outwards and clips to the target.
pub(crate) fn physical_scissors(
    damage: &[Rect],
    scale_factor: f32,
    target: (u32, u32),
) -> Vec<ScissorRect> {
    let bounds = Rect::from_size(Size::new(target.0 as f32, target.1 as f32));
    damage
        .iter()
        .filter_map(|region| region.scale(scale_factor).round_out().intersection(&bounds))
        .filter(|region| !region.is_empty())
        .map(|region| ScissorRect {
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
        let scissors = physical_scissors(&[Rect::new(10.2, 5.5, 20.1, 4.0)], 2.0, (800, 600));
        assert_eq!(
            scissors,
            [ScissorRect {
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
        let scissors = physical_scissors(&damage, 1.0, (800, 600));
        assert_eq!(
            scissors,
            [ScissorRect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            }]
        );
    }
}
