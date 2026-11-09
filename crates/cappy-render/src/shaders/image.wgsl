// An image quad clipped to a rounded box. UVs map `dest` onto a window of the texture.

struct Globals {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(2) var<uniform> globals: Globals;

struct ImageInput {
    @location(0) clip_rect: vec4<f32>,
    @location(1) dest_rect: vec4<f32>,
    @location(2) uv_rect: vec4<f32>,
    @location(3) radius_opacity: vec2<f32>,
    @location(4) scroll_clip: vec4<f32>,
}

struct ImageOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) pixel: vec2<f32>,
    @location(1) clip_rect: vec4<f32>,
    @location(2) dest_rect: vec4<f32>,
    @location(3) uv_rect: vec4<f32>,
    @location(4) radius: f32,
    @location(5) opacity: f32,
    @location(6) scroll_clip: vec4<f32>,
}

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex
fn vs_image(@builtin(vertex_index) vertex_index: u32, image: ImageInput) -> ImageOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let margin = 1.0;
    let origin = image.dest_rect.xy - vec2<f32>(margin);
    let extent = image.dest_rect.zw + vec2<f32>(2.0 * margin);
    let pixel = origin + corners[vertex_index] * extent;
    let clip = vec2<f32>(
        pixel.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.viewport.y * 2.0,
    );

    var out: ImageOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.pixel = pixel;
    out.clip_rect = image.clip_rect;
    out.dest_rect = image.dest_rect;
    out.uv_rect = image.uv_rect;
    out.radius = image.radius_opacity.x;
    out.opacity = image.radius_opacity.y;
    out.scroll_clip = image.scroll_clip;
    return out;
}

fn scroll_coverage(pixel: vec2<f32>, clip: vec4<f32>) -> f32 {
    let end = clip.xy + clip.zw;
    let inside = step(clip.xy, pixel) * step(pixel, end);
    return inside.x * inside.y;
}

fn image_color(in: ImageOutput) -> vec4<f32> {
    let half_size = in.clip_rect.zw * 0.5;
    let local = in.pixel - (in.clip_rect.xy + half_size);
    let radius = min(in.radius, min(half_size.x, half_size.y));
    let coverage = clamp(0.5 - rounded_box_distance(local, half_size, radius), 0.0, 1.0);
    let dest_span = max(in.dest_rect.zw, vec2<f32>(1e-4));
    let dest_uv = (in.pixel - in.dest_rect.xy) / dest_span;
    let inside = step(vec2<f32>(0.0), dest_uv) * step(dest_uv, vec2<f32>(1.0));
    let uv = mix(in.uv_rect.xy, in.uv_rect.zw, clamp(dest_uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    let sample = textureSample(image_texture, image_sampler, uv);
    let premul = vec4<f32>(sample.rgb * sample.a, sample.a);
    return premul * coverage * inside.x * inside.y * scroll_coverage(in.pixel, in.scroll_clip) * in.opacity;
}

@fragment
fn fs_image(in: ImageOutput) -> @location(0) vec4<f32> {
    return image_color(in);
}

@fragment
fn fs_image_encode_srgb(in: ImageOutput) -> @location(0) vec4<f32> {
    return encode_srgb(image_color(in));
}
