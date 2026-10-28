//! Splits the damaged part of a display list into draw batches.
//!
//! Quads and text use different pipelines. Within a batch all quads are drawn first, then all
//! text, which is only correct while no quad covers earlier text; a quad that does starts a new
//! batch.

use cappy_core::{Primitive, Rect};

/// Anti-aliasing can bleed slightly past a primitive's bounds.
const CULL_MARGIN: f32 = 1.0;

/// Indices into the display list, drawn quads first.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Batch {
    pub quads: Vec<usize>,
    pub texts: Vec<usize>,
}

/// Batches for the primitives that touch any of `damage` (logical pixels), in paint order.
pub(crate) fn plan_batches(primitives: &[Primitive], damage: &[Rect]) -> Vec<Batch> {
    let mut batches = vec![Batch::default()];
    for (index, primitive) in primitives.iter().enumerate() {
        let bounds = primitive.bounds().inflate(CULL_MARGIN);
        if !damage.iter().any(|region| region.intersects(&bounds)) {
            continue;
        }
        match primitive {
            Primitive::Quad(_) => {
                if covers_batch_text(primitives, current(&batches), &bounds) {
                    batches.push(Batch::default());
                }
                current_mut(&mut batches).quads.push(index);
            }
            Primitive::Text(_) => current_mut(&mut batches).texts.push(index),
        }
    }
    batches.retain(|batch| !batch.quads.is_empty() || !batch.texts.is_empty());
    batches
}

fn covers_batch_text(primitives: &[Primitive], batch: &Batch, bounds: &Rect) -> bool {
    batch
        .texts
        .iter()
        .any(|&text| primitives[text].bounds().intersects(bounds))
}

fn current(batches: &[Batch]) -> &Batch {
    batches.last().expect("plan_batches always keeps one batch")
}

fn current_mut(batches: &mut [Batch]) -> &mut Batch {
    batches
        .last_mut()
        .expect("plan_batches always keeps one batch")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cappy_core::{Color, QuadPrimitive, TextPrimitive};

    use super::*;

    fn quad(x: f32, y: f32, size: f32) -> Primitive {
        Primitive::Quad(QuadPrimitive {
            rect: Rect::new(x, y, size, size),
            background: Color::WHITE,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            corner_radius: 0.0,
        })
    }

    fn label(x: f32, y: f32) -> Primitive {
        Primitive::Text(TextPrimitive {
            rect: Rect::new(x, y, 20.0, 10.0),
            text: Arc::from("label"),
            color: Color::BLACK,
            font_size: 10.0,
            line_height: 10.0,
            weight: 400,
        })
    }

    const EVERYTHING: [Rect; 1] = [Rect::new(0.0, 0.0, 1000.0, 1000.0)];

    #[test]
    fn disjoint_quads_and_text_share_one_batch() {
        let primitives = [
            quad(0.0, 0.0, 100.0),
            label(10.0, 10.0),
            quad(200.0, 0.0, 50.0),
        ];
        let batches = plan_batches(&primitives, &EVERYTHING);
        assert_eq!(
            batches,
            [Batch {
                quads: vec![0, 2],
                texts: vec![1],
            }]
        );
    }

    #[test]
    fn quad_over_earlier_text_starts_a_new_batch() {
        let primitives = [label(10.0, 10.0), quad(0.0, 0.0, 100.0), label(20.0, 20.0)];
        let batches = plan_batches(&primitives, &EVERYTHING);
        assert_eq!(
            batches,
            [
                Batch {
                    quads: vec![],
                    texts: vec![0],
                },
                Batch {
                    quads: vec![1],
                    texts: vec![2],
                },
            ]
        );
    }

    #[test]
    fn primitives_outside_the_damage_are_skipped() {
        let primitives = [quad(0.0, 0.0, 10.0), quad(500.0, 500.0, 10.0)];
        let batches = plan_batches(&primitives, &[Rect::new(400.0, 400.0, 200.0, 200.0)]);
        assert_eq!(
            batches,
            [Batch {
                quads: vec![1],
                texts: vec![],
            }]
        );
    }

    #[test]
    fn no_damage_means_no_batches() {
        assert!(plan_batches(&[quad(0.0, 0.0, 10.0)], &[]).is_empty());
    }
}
