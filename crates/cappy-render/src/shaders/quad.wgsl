// Rounded, bordered rectangles drawn as instanced quads with an SDF edge.
// Positions are physical pixels; colors are linear and premultiplied.

struct Globals {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct QuadInput {
    @location(0) rect: vec4<f32>,
    @location(1) background: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    // x: border width, y: corner radius.
    @location(3) shape: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) background: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) shape: vec2<f32>,
}

// Room for the anti-aliased edge outside the rectangle.
const EDGE_MARGIN: f32 = 1.0;

@vertex
fn vs_quad(@builtin(vertex_index) vertex_index: u32, quad: QuadInput) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let origin = quad.rect.xy - vec2<f32>(EDGE_MARGIN);
    let extent = quad.rect.zw + vec2<f32>(2.0 * EDGE_MARGIN);
    let pixel = origin + corners[vertex_index] * extent;
    let clip = vec2<f32>(
        pixel.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.viewport.y * 2.0,
    );

    var out: VertexOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.half_size = quad.rect.zw * 0.5;
    out.local = pixel - (quad.rect.xy + out.half_size);
    out.background = quad.background;
    out.border_color = quad.border_color;
    out.shape = quad.shape;
    return out;
}

fn rounded_box_distance(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let corner = abs(point) - half_size + vec2<f32>(radius);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - radius;
}

fn quad_color(in: VertexOutput) -> vec4<f32> {
    let radius = min(in.shape.y, min(in.half_size.x, in.half_size.y));
    let coverage = clamp(0.5 - rounded_box_distance(in.local, in.half_size, radius), 0.0, 1.0);
    let border_width = in.shape.x;
    var color = in.background;
    if border_width > 0.0 {
        let inner_half = max(in.half_size - vec2<f32>(border_width), vec2<f32>(0.0));
        let inner_radius = max(radius - border_width, 0.0);
        let inner_distance = rounded_box_distance(in.local, inner_half, inner_radius);
        let inside = clamp(0.5 - inner_distance, 0.0, 1.0);
        let border_over_background = in.border_color + in.background * (1.0 - in.border_color.a);
        color = mix(border_over_background, in.background, inside);
    }
    return color * coverage;
}

@fragment
fn fs_quad(in: VertexOutput) -> @location(0) vec4<f32> {
    return quad_color(in);
}

@fragment
fn fs_quad_encode_srgb(in: VertexOutput) -> @location(0) vec4<f32> {
    return encode_srgb(quad_color(in));
}
