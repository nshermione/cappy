// A tessellated path clipped to a rounded box. `edge` is fill coverage, or a signed distance
// from a stroke centerline when `limit` is the stroke's half width.

struct Globals {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) edge: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) clip_rect: vec4<f32>,
    @location(4) dest_rect: vec4<f32>,
    @location(5) scroll_clip: vec4<f32>,
    @location(6) radius: f32,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) pixel: vec2<f32>,
    @location(2) edge: vec2<f32>,
    @location(3) clip_rect: vec4<f32>,
    @location(4) dest_rect: vec4<f32>,
    @location(5) scroll_clip: vec4<f32>,
    @location(6) radius: f32,
}

@vertex
fn vs_vector(in: VertexIn) -> VertexOut {
    let clip = vec2<f32>(
        in.position.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - in.position.y / globals.viewport.y * 2.0,
    );
    var out: VertexOut;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.color = in.color;
    out.pixel = in.position;
    out.edge = in.edge;
    out.clip_rect = in.clip_rect;
    out.dest_rect = in.dest_rect;
    out.scroll_clip = in.scroll_clip;
    out.radius = in.radius;
    return out;
}

fn scroll_coverage(pixel: vec2<f32>, clip: vec4<f32>) -> f32 {
    let end = clip.xy + clip.zw;
    let inside = step(clip.xy, pixel) * step(pixel, end);
    return inside.x * inside.y;
}

fn vector_color(in: VertexOut) -> vec4<f32> {
    let half_size = in.clip_rect.zw * 0.5;
    let local = in.pixel - (in.clip_rect.xy + half_size);
    let radius = min(in.radius, min(half_size.x, half_size.y));
    let box_coverage = clamp(0.5 - rounded_box_distance(local, half_size, radius), 0.0, 1.0);
    let dest_end = in.dest_rect.xy + in.dest_rect.zw;
    let inside = step(in.dest_rect.xy, in.pixel) * step(in.pixel, dest_end);
    let fill_aa = clamp(in.edge.x, 0.0, 1.0);
    let stroke_aa = clamp(in.edge.y - abs(in.edge.x) + 0.5, 0.0, 1.0);
    let edge = select(fill_aa, stroke_aa, in.edge.y > 0.0);
    return in.color * box_coverage * inside.x * inside.y * scroll_coverage(in.pixel, in.scroll_clip) * edge;
}

@fragment
fn fs_vector(in: VertexOut) -> @location(0) vec4<f32> {
    return vector_color(in);
}

@fragment
fn fs_vector_encode_srgb(in: VertexOut) -> @location(0) vec4<f32> {
    return encode_srgb(vector_color(in));
}
