// Rounded rectangles, linear gradients and drop shadows, drawn as instanced quads.
// Positions are physical pixels; colors are linear and premultiplied.
// shape: shared border width, shared corner radius, blur radius, mode (0 fill, 1 shadow).
// borders: top, right, bottom, left. radii: top-left, top-right, bottom-right, bottom-left.
// Equal borders and radii use the shared shape values, so a uniform box matches the old SDF.
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
    @location(6) clip: vec4<f32>,
    // pivot.xy in physical pixels, clockwise radians, unused.
    @location(7) spin: vec4<f32>,
    @location(8) borders: vec4<f32>,
    @location(9) radii: vec4<f32>,
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
    @location(7) pixel: vec2<f32>,
    @location(8) clip: vec4<f32>,
    @location(9) spin: vec4<f32>,
    @location(10) rect_origin: vec2<f32>,
    @location(11) borders: vec4<f32>,
    @location(12) radii: vec4<f32>,
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
    var pixel = origin + corners[vertex_index] * extent;
    if abs(quad.spin.z) > 1e-5 {
        // Cover every fragment of the rotated box, including the blur margin.
        var min_p = vec2<f32>(1e9);
        var max_p = vec2<f32>(-1e9);
        for (var i = 0u; i < 4u; i = i + 1u) {
            let unit = vec2<f32>(f32(i % 2u), f32(i / 2u));
            let spun = rotate_about(origin + unit * extent, quad.spin.xy, quad.spin.z);
            min_p = min(min_p, spun);
            max_p = max(max_p, spun);
        }
        pixel = min_p + corners[vertex_index] * (max_p - min_p);
    }
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
    out.pixel = pixel;
    out.clip = quad.clip;
    out.spin = quad.spin;
    out.rect_origin = quad.rect.xy;
    out.borders = quad.borders;
    out.radii = quad.radii;
    return out;
}

fn rotate_about(point: vec2<f32>, pivot: vec2<f32>, angle: f32) -> vec2<f32> {
    let delta = point - pivot;
    let c = cos(angle);
    let s = sin(angle);
    return pivot + vec2<f32>(c * delta.x - s * delta.y, s * delta.x + c * delta.y);
}

fn surface_local(in: VertexOutput) -> vec2<f32> {
    if abs(in.spin.z) <= 1e-5 {
        return in.local;
    }
    let delta = in.pixel - in.spin.xy;
    let c = cos(-in.spin.z);
    let s = sin(-in.spin.z);
    let pixel = in.spin.xy + vec2<f32>(c * delta.x - s * delta.y, s * delta.x + c * delta.y);
    return pixel - (in.rect_origin + in.half_size);
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
    let uv = surface_local(in) / size + vec2<f32>(0.5);
    let t = clamp(dot(uv - in.gradient.xy, delta) / length_squared, 0.0, 1.0);
    return mix(in.background, in.color_to, t);
}

fn clip_coverage(pixel: vec2<f32>, clip: vec4<f32>) -> f32 {
    let end = clip.xy + clip.zw;
    let inside = step(clip.xy, pixel) * step(pixel, end);
    return inside.x * inside.y;
}

fn sides_match(value: vec4<f32>) -> bool {
    return value.x == value.y && value.x == value.z && value.x == value.w;
}

fn corner_radius_at(point: vec2<f32>, radii: vec4<f32>) -> f32 {
    // y grows downward. radii is top-left, top-right, bottom-right, bottom-left.
    if point.y < 0.0 {
        return select(radii.x, radii.y, point.x >= 0.0);
    }
    return select(radii.w, radii.z, point.x >= 0.0);
}

fn rounded_box_distance_corners(point: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let radius = min(corner_radius_at(point, radii), min(half_size.x, half_size.y));
    let corner = abs(point) - half_size + vec2<f32>(radius);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - radius;
}

fn quad_color(in: VertexOutput) -> vec4<f32> {
    let local = surface_local(in);
    let uniform = sides_match(in.borders) && sides_match(in.radii);
    if uniform {
        let radius = min(in.shape.y, min(in.half_size.x, in.half_size.y));
        if in.shape.w > 0.5 {
            let distance = rounded_box_distance(local, in.half_size, radius);
            return in.background * shadow_coverage(distance, in.shape.z) * clip_coverage(in.pixel, in.clip);
        }
        let coverage = clamp(0.5 - rounded_box_distance(local, in.half_size, radius), 0.0, 1.0);
        let border_width = in.shape.x;
        var color = fill_color(in);
        if border_width > 0.0 {
            let inner_half = max(in.half_size - vec2<f32>(border_width), vec2<f32>(0.0));
            let inner_radius = max(radius - border_width, 0.0);
            let inner_distance = rounded_box_distance(local, inner_half, inner_radius);
            let inside = clamp(0.5 - inner_distance, 0.0, 1.0);
            let border_over_background = in.border_color + color * (1.0 - in.border_color.a);
            color = mix(border_over_background, color, inside);
        }
        return color * coverage * clip_coverage(in.pixel, in.clip);
    }
    if in.shape.w > 0.5 {
        let distance = rounded_box_distance_corners(local, in.half_size, in.radii);
        return in.background * shadow_coverage(distance, in.shape.z) * clip_coverage(in.pixel, in.clip);
    }
    let coverage = clamp(0.5 - rounded_box_distance_corners(local, in.half_size, in.radii), 0.0, 1.0);
    var color = fill_color(in);
    if in.borders.x > 0.0 || in.borders.y > 0.0 || in.borders.z > 0.0 || in.borders.w > 0.0 {
        // borders: top, right, bottom, left. The inner box shifts when opposite edges differ.
        let shift = vec2<f32>((in.borders.w - in.borders.y) * 0.5, (in.borders.x - in.borders.z) * 0.5);
        let inner_half = max(
            in.half_size - vec2<f32>((in.borders.y + in.borders.w) * 0.5, (in.borders.x + in.borders.z) * 0.5),
            vec2<f32>(0.0),
        );
        let inner_radii = max(vec4<f32>(
            in.radii.x - max(in.borders.x, in.borders.w),
            in.radii.y - max(in.borders.x, in.borders.y),
            in.radii.z - max(in.borders.z, in.borders.y),
            in.radii.w - max(in.borders.z, in.borders.w),
        ), vec4<f32>(0.0));
        let inner_distance = rounded_box_distance_corners(local - shift, inner_half, inner_radii);
        let inside = clamp(0.5 - inner_distance, 0.0, 1.0);
        let border_over_background = in.border_color + color * (1.0 - in.border_color.a);
        color = mix(border_over_background, color, inside);
    }
    return color * coverage * clip_coverage(in.pixel, in.clip);
}

@fragment
fn fs_quad(in: VertexOutput) -> @location(0) vec4<f32> {
    return quad_color(in);
}

@fragment
fn fs_quad_encode_srgb(in: VertexOutput) -> @location(0) vec4<f32> {
    return encode_srgb(quad_color(in));
}
