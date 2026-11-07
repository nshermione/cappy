// Rounded rectangles, linear gradients and drop shadows, drawn as instanced quads.
// Positions are physical pixels; colors are linear and premultiplied.
// shape: border width, corner radius, blur radius, mode (0 fill, 1 shadow).
// gradient: start.xy, end.xy in the unit box. A zero-length segment is a solid color.

struct Globals {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct QuadInput {
    @location(0) rect: vec4<f32>,
    @location(1) background: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    @location(3) color_to: vec4<f32>,
    @location(4) shape: vec4<f32>,
    @location(5) gradient: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) background: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) color_to: vec4<f32>,
    @location(5) shape: vec4<f32>,
    @location(6) gradient: vec4<f32>,
}

@vertex
fn vs_quad(@builtin(vertex_index) vertex_index: u32, quad: QuadInput) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    // Blur paints 1.5 radii (3 standard deviations) past the shape. Fills keep a 1px edge.
    let margin = 1.0 + quad.shape.z * 1.5;
    let origin = quad.rect.xy - vec2<f32>(margin);
    let extent = quad.rect.zw + vec2<f32>(2.0 * margin);
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
    out.color_to = quad.color_to;
    out.shape = quad.shape;
    out.gradient = quad.gradient;
    return out;
}

// Abramowitz and Stegun 7.1.26. Matches BoxShadow::coverage in cappy-style.
fn erf_approx(x: f32) -> f32 {
    let sign = select(-1.0, 1.0, x >= 0.0);
    let a = abs(x);
    let t = 1.0 / (1.0 + 0.3275911 * a);
    let polynomial = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t;
    return sign * (1.0 - polynomial * exp(-a * a));
}

fn shadow_coverage(distance: f32, blur: f32) -> f32 {
    if blur <= 0.0 {
        return clamp(0.5 - distance, 0.0, 1.0);
    }
    let sigma = blur * 0.5;
    let x = distance / (sigma * 1.41421356);
    return 0.5 * (1.0 - erf_approx(x));
}

fn fill_color(in: VertexOutput) -> vec4<f32> {
    let delta = in.gradient.zw - in.gradient.xy;
    let length_squared = dot(delta, delta);
    if length_squared <= 1e-6 {
        return in.background;
    }
    let size = max(in.half_size * 2.0, vec2<f32>(1e-4));
    let uv = in.local / size + vec2<f32>(0.5);
    let t = clamp(dot(uv - in.gradient.xy, delta) / length_squared, 0.0, 1.0);
    return mix(in.background, in.color_to, t);
}

fn quad_color(in: VertexOutput) -> vec4<f32> {
    let radius = min(in.shape.y, min(in.half_size.x, in.half_size.y));
    if in.shape.w > 0.5 {
        let distance = rounded_box_distance(in.local, in.half_size, radius);
        return in.background * shadow_coverage(distance, in.shape.z);
    }
    let coverage = clamp(0.5 - rounded_box_distance(in.local, in.half_size, radius), 0.0, 1.0);
    let border_width = in.shape.x;
    var color = fill_color(in);
    if border_width > 0.0 {
        let inner_half = max(in.half_size - vec2<f32>(border_width), vec2<f32>(0.0));
        let inner_radius = max(radius - border_width, 0.0);
        let inner_distance = rounded_box_distance(in.local, inner_half, inner_radius);
        let inside = clamp(0.5 - inner_distance, 0.0, 1.0);
        let border_over_background = in.border_color + color * (1.0 - in.border_color.a);
        color = mix(border_over_background, color, inside);
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
