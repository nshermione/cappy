//! A picture made of filled and stroked paths. SVG decoding lowers a supported file into one
//! [`VectorPicture`]; the GPU renderer tessellates it at the size being drawn.

use std::sync::Arc;

use cappy_layout::Point;
use cappy_style::Color;

use crate::image_decode::ImageDecodeError;

/// One segment of a [`VectorPath`]. Point counts are `Move` 1, `Line` 1, `Quad` 2, `Cubic` 3,
/// `Close` 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathVerb {
    Move,
    Line,
    Quad,
    Cubic,
    Close,
}

/// How a filled path decides which side is inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// Cap of an open stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// Join between two stroke segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    MiterClip,
    Round,
    Bevel,
}

/// Bezier path in viewBox space. Transforms from the SVG tree are already baked in.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    pub verbs: Arc<[PathVerb]>,
    pub points: Arc<[Point]>,
}

/// Color of a fill or stroke, in the same space as [`VectorPath`].
#[derive(Clone, Debug, PartialEq)]
pub enum VectorPaint {
    Solid(Color),
    /// `stops` are `(offset, color)` along the segment from `start` to `end`. Offsets run from 0
    /// to 1. The renderer clamps the parameter to that range.
    Linear {
        start: Point,
        end: Point,
        stops: Arc<[(f32, Color)]>,
    },
}

/// Fill or stroke applied to one path.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorStyle {
    Fill {
        paint: VectorPaint,
        rule: FillRule,
    },
    Stroke {
        paint: VectorPaint,
        /// Width in viewBox units, after the path's own transform.
        width: f32,
        cap: LineCap,
        join: LineJoin,
        miter_limit: f32,
    },
}

/// One painted path. `opacity` multiplies the paint.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorDraw {
    pub path: VectorPath,
    pub style: VectorStyle,
    pub opacity: f32,
}

/// Immutable vector picture shared across frames. `view_width` and `view_height` are the SVG
/// viewport the paths are measured in.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPicture {
    pub view_width: f32,
    pub view_height: f32,
    pub draws: Arc<[VectorDraw]>,
}

/// `Ok(None)` means the file parsed but uses something this picture cannot draw, so the caller
/// rasterizes it instead.
pub(crate) fn decode_picture(bytes: &[u8]) -> Result<Option<VectorPicture>, ImageDecodeError> {
    let options = resvg::usvg::Options {
        default_size: resvg::usvg::Size::from_wh(512.0, 512.0)
            .expect("512 is a positive svg default size"),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(failed)?;
    match lower(&tree) {
        Ok(picture) => Ok(Some(picture)),
        Err(Unsupported) => Ok(None),
    }
}

fn failed(error: impl std::fmt::Display) -> ImageDecodeError {
    ImageDecodeError::Failed(format!("failed to decode image: {error}"))
}

struct Unsupported;

fn lower(tree: &resvg::usvg::Tree) -> Result<VectorPicture, Unsupported> {
    let mut draws = Vec::new();
    walk(tree.root(), 1.0, &mut draws)?;
    let size = tree.size();
    if !(size.width().is_finite()
        && size.height().is_finite()
        && size.width() > 0.0
        && size.height() > 0.0)
    {
        return Err(Unsupported);
    }
    Ok(VectorPicture {
        view_width: size.width(),
        view_height: size.height(),
        draws: Arc::from(draws),
    })
}

fn walk(
    group: &resvg::usvg::Group,
    parent_opacity: f32,
    draws: &mut Vec<VectorDraw>,
) -> Result<(), Unsupported> {
    if group.clip_path().is_some()
        || group.mask().is_some()
        || !group.filters().is_empty()
        || group.blend_mode() != resvg::usvg::BlendMode::Normal
        || group.isolate()
    {
        return Err(Unsupported);
    }
    let opacity = parent_opacity * group.opacity().get();
    if group.opacity().get() < 1.0 - 1.0e-3 && children_overlap(group) {
        return Err(Unsupported);
    }
    for child in group.children() {
        match child {
            resvg::usvg::Node::Group(group) => walk(group, opacity, draws)?,
            resvg::usvg::Node::Path(path) => push_path(path, opacity, draws)?,
            resvg::usvg::Node::Image(_) | resvg::usvg::Node::Text(_) => return Err(Unsupported),
        }
    }
    Ok(())
}

fn children_overlap(group: &resvg::usvg::Group) -> bool {
    let boxes: Vec<resvg::usvg::Rect> = group
        .children()
        .iter()
        .map(resvg::usvg::Node::abs_bounding_box)
        .collect();
    for left in 0..boxes.len() {
        for right in (left + 1)..boxes.len() {
            if boxes[left].left() < boxes[right].right()
                && boxes[right].left() < boxes[left].right()
                && boxes[left].top() < boxes[right].bottom()
                && boxes[right].top() < boxes[left].bottom()
            {
                return true;
            }
        }
    }
    false
}

fn push_path(
    path: &resvg::usvg::Path,
    opacity: f32,
    draws: &mut Vec<VectorDraw>,
) -> Result<(), Unsupported> {
    if !path.is_visible() {
        return Ok(());
    }
    let geometry = bake_path(path)?;
    let mut push = |style| {
        draws.push(VectorDraw {
            path: geometry.clone(),
            style,
            opacity,
        });
    };
    let fill_first = path.paint_order() != resvg::usvg::PaintOrder::StrokeAndFill;
    if fill_first {
        if let Some(style) = fill_style(path)? {
            push(style);
        }
        if let Some(style) = stroke_style(path)? {
            push(style);
        }
    } else {
        if let Some(style) = stroke_style(path)? {
            push(style);
        }
        if let Some(style) = fill_style(path)? {
            push(style);
        }
    }
    Ok(())
}

fn fill_style(path: &resvg::usvg::Path) -> Result<Option<VectorStyle>, Unsupported> {
    let Some(fill) = path.fill() else {
        return Ok(None);
    };
    // A horizontal or vertical line has no area. resvg skips that fill.
    let bounds = path.data().bounds();
    if bounds.width() == 0.0 || bounds.height() == 0.0 {
        return Ok(None);
    }
    Ok(Some(VectorStyle::Fill {
        paint: convert_paint(fill.paint(), path.abs_transform(), fill.opacity().get())?,
        rule: match fill.rule() {
            resvg::usvg::FillRule::NonZero => FillRule::NonZero,
            resvg::usvg::FillRule::EvenOdd => FillRule::EvenOdd,
        },
    }))
}

fn stroke_style(path: &resvg::usvg::Path) -> Result<Option<VectorStyle>, Unsupported> {
    let Some(stroke) = path.stroke() else {
        return Ok(None);
    };
    if stroke.dasharray().is_some() {
        return Err(Unsupported);
    }
    let (scale_x, scale_y) = path.abs_transform().get_scale();
    Ok(Some(VectorStyle::Stroke {
        paint: convert_paint(stroke.paint(), path.abs_transform(), stroke.opacity().get())?,
        width: stroke.width().get() * (scale_x + scale_y) * 0.5,
        cap: match stroke.linecap() {
            resvg::usvg::LineCap::Butt => LineCap::Butt,
            resvg::usvg::LineCap::Round => LineCap::Round,
            resvg::usvg::LineCap::Square => LineCap::Square,
        },
        join: match stroke.linejoin() {
            resvg::usvg::LineJoin::Miter => LineJoin::Miter,
            resvg::usvg::LineJoin::MiterClip => LineJoin::MiterClip,
            resvg::usvg::LineJoin::Round => LineJoin::Round,
            resvg::usvg::LineJoin::Bevel => LineJoin::Bevel,
        },
        miter_limit: stroke.miterlimit().get(),
    }))
}

fn convert_paint(
    paint: &resvg::usvg::Paint,
    path_transform: resvg::tiny_skia::Transform,
    opacity: f32,
) -> Result<VectorPaint, Unsupported> {
    match paint {
        resvg::usvg::Paint::Color(color) => Ok(VectorPaint::Solid(color_with_alpha(
            color.red,
            color.green,
            color.blue,
            opacity,
        ))),
        resvg::usvg::Paint::LinearGradient(gradient) => {
            if gradient.spread_method() != resvg::usvg::SpreadMethod::Pad
                || gradient.stops().is_empty()
            {
                return Err(Unsupported);
            }
            let start = map_gradient_point(
                gradient.x1(),
                gradient.y1(),
                gradient.transform(),
                path_transform,
            );
            let end = map_gradient_point(
                gradient.x2(),
                gradient.y2(),
                gradient.transform(),
                path_transform,
            );
            let mut stops: Vec<(f32, Color)> = gradient
                .stops()
                .iter()
                .map(|stop| {
                    (
                        stop.offset().get(),
                        color_with_alpha(
                            stop.color().red,
                            stop.color().green,
                            stop.color().blue,
                            stop.opacity().get() * opacity,
                        ),
                    )
                })
                .collect();
            if stops.len() == 1 {
                stops.push(stops[0]);
            }
            Ok(VectorPaint::Linear {
                start,
                end,
                stops: Arc::from(stops),
            })
        }
        resvg::usvg::Paint::RadialGradient(_) | resvg::usvg::Paint::Pattern(_) => Err(Unsupported),
    }
}

fn map_gradient_point(
    x: f32,
    y: f32,
    gradient: resvg::tiny_skia::Transform,
    path: resvg::tiny_skia::Transform,
) -> Point {
    let mut point = resvg::tiny_skia::Point::from_xy(x, y);
    gradient.map_point(&mut point);
    path.map_point(&mut point);
    Point::new(point.x, point.y)
}

fn color_with_alpha(red: u8, green: u8, blue: u8, opacity: f32) -> Color {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgba(red, green, blue, alpha)
}

fn bake_path(path: &resvg::usvg::Path) -> Result<VectorPath, Unsupported> {
    let baked = path
        .data()
        .clone()
        .transform(path.abs_transform())
        .ok_or(Unsupported)?;
    let verbs = baked
        .verbs()
        .iter()
        .copied()
        .map(|verb| match verb {
            resvg::usvg::tiny_skia_path::PathVerb::Move => PathVerb::Move,
            resvg::usvg::tiny_skia_path::PathVerb::Line => PathVerb::Line,
            resvg::usvg::tiny_skia_path::PathVerb::Quad => PathVerb::Quad,
            resvg::usvg::tiny_skia_path::PathVerb::Cubic => PathVerb::Cubic,
            resvg::usvg::tiny_skia_path::PathVerb::Close => PathVerb::Close,
        })
        .collect::<Vec<_>>();
    let points = baked
        .points()
        .iter()
        .map(|point| Point::new(point.x, point.y))
        .collect::<Vec<_>>();
    Ok(VectorPath {
        verbs: Arc::from(verbs),
        points: Arc::from(points),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(bytes: &[u8]) -> Option<VectorPicture> {
        decode_picture(bytes).expect("svg parses")
    }

    #[test]
    fn sample_svg_becomes_three_solid_fills() {
        let picture = picture(include_bytes!("../fixtures/sample.svg")).expect("paths");
        assert_eq!((picture.view_width, picture.view_height), (64.0, 48.0));
        assert_eq!(picture.draws.len(), 3);
        let colors = [
            Color::hex("#38bdf8"),
            Color::hex("#fbbf24"),
            Color::hex("#22c55e"),
        ];
        for (draw, color) in picture.draws.iter().zip(colors) {
            match &draw.style {
                VectorStyle::Fill {
                    paint: VectorPaint::Solid(paint),
                    rule: FillRule::NonZero,
                } => assert_eq!(*paint, color),
                other => panic!("expected a solid fill, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_translation_is_baked_into_the_path() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><g transform="translate(5,0)"><rect width="4" height="10" fill="black"/></g></svg>"#;
        let picture = picture(svg).expect("paths");
        let points = &picture.draws[0].path.points;
        assert!(points.iter().any(|point| point.x >= 4.0));
        assert!(points.iter().all(|point| point.x >= 4.0));
    }

    #[test]
    fn a_stroke_keeps_width_cap_and_join() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><line x1="1" y1="1" x2="9" y2="1" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
        let picture = picture(svg).expect("paths");
        let VectorStyle::Stroke {
            width, cap, join, ..
        } = &picture.draws[0].style
        else {
            panic!("line is a stroke");
        };
        assert!((width - 2.0).abs() < 0.01);
        assert_eq!(*cap, LineCap::Round);
        assert_eq!(*join, LineJoin::Miter);
    }

    #[test]
    fn a_dash_a_radial_gradient_and_a_filter_fall_back() {
        let dashed = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><line x1="0" y1="5" x2="10" y2="5" stroke="black" stroke-dasharray="2 2"/></svg>"#;
        let radial = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><defs><radialGradient id="g"><stop offset="0" stop-color="white"/><stop offset="1" stop-color="black"/></radialGradient></defs><circle cx="5" cy="5" r="4" fill="url(#g)"/></svg>"#;
        let filtered = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><defs><filter id="f"><feGaussianBlur stdDeviation="1"/></filter></defs><rect width="10" height="10" filter="url(#f)"/></svg>"#;
        assert!(picture(dashed).is_none());
        assert!(picture(radial).is_none());
        assert!(picture(filtered).is_none());
    }

    #[test]
    fn a_linear_gradient_keeps_its_stops() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><defs><linearGradient id="g" x1="0" y1="0" x2="10" y2="0"><stop offset="0" stop-color="black"/><stop offset="1" stop-color="white"/></linearGradient></defs><rect width="10" height="10" fill="url(#g)"/></svg>"#;
        let picture = picture(svg).expect("paths");
        let VectorStyle::Fill {
            paint: VectorPaint::Linear { stops, .. },
            ..
        } = &picture.draws[0].style
        else {
            panic!("expected a linear fill");
        };
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].1, Color::BLACK);
        assert_eq!(stops[1].1, Color::WHITE);
    }
}
