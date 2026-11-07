// Signed distance to a rounded rectangle. `point` is relative to the center.
// Negative values are inside the shape.
fn rounded_box_distance(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let clamped = min(radius, min(half_size.x, half_size.y));
    let corner = abs(point) - half_size + vec2<f32>(clamped);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - clamped;
}
