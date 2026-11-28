// A video quad. Untagged H.264 is studio-range BT.601: luma 16–235, chroma 16–240.
// The matrix result is already display gamma. An sRGB target must receive linear light.

struct Globals {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(4) var<uniform> globals: Globals;

struct VideoInput {
    @location(0) clip_rect: vec4<f32>,
    @location(1) dest_rect: vec4<f32>,
    @location(2) uv_rect: vec4<f32>,
    @location(3) radius_opacity: vec4<f32>,
    @location(4) scroll_clip: vec4<f32>,
}

struct VideoOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) pixel: vec2<f32>,
    @location(1) clip_rect: vec4<f32>,
    @location(2) dest_rect: vec4<f32>,
    @location(3) uv_rect: vec4<f32>,
    @location(4) radius: f32,
    @location(5) opacity: f32,
    @location(6) scroll_clip: vec4<f32>,
}

@group(0) @binding(0) var plane_y: texture_2d<f32>;
@group(0) @binding(1) var plane_u: texture_2d<f32>;
@group(0) @binding(2) var plane_v: texture_2d<f32>;
@group(0) @binding(3) var plane_sampler: sampler;

@vertex
fn vs_video(@builtin(vertex_index) vertex_index: u32, video: VideoInput) -> VideoOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let margin = 1.0;
    let origin = video.dest_rect.xy - vec2<f32>(margin);
    let extent = video.dest_rect.zw + vec2<f32>(2.0 * margin);
    let pixel = origin + corners[vertex_index] * extent;
    let clip = vec2<f32>(
        pixel.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.viewport.y * 2.0,
    );

    var out: VideoOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.pixel = pixel;
    out.clip_rect = video.clip_rect;
    out.dest_rect = video.dest_rect;
    out.uv_rect = video.uv_rect;
    out.radius = video.radius_opacity.x;
    out.opacity = video.radius_opacity.y;
    out.scroll_clip = video.scroll_clip;
    return out;
}

fn scroll_coverage(pixel: vec2<f32>, clip: vec4<f32>) -> f32 {
    let end = clip.xy + clip.zw;
    let inside = step(clip.xy, pixel) * step(pixel, end);
    return inside.x * inside.y;
}

fn video_color(in: VideoOutput) -> vec4<f32> {
    let half_size = in.clip_rect.zw * 0.5;
    let local = in.pixel - (in.clip_rect.xy + half_size);
    let radius = min(in.radius, min(half_size.x, half_size.y));
    let coverage = clamp(0.5 - rounded_box_distance(local, half_size, radius), 0.0, 1.0);
    let dest_span = max(in.dest_rect.zw, vec2<f32>(1e-4));
    let dest_uv = (in.pixel - in.dest_rect.xy) / dest_span;
    let inside = step(vec2<f32>(0.0), dest_uv) * step(dest_uv, vec2<f32>(1.0));
    let uv = mix(in.uv_rect.xy, in.uv_rect.zw, clamp(dest_uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    let y = (textureSample(plane_y, plane_sampler, uv).r - 16.0 / 255.0) * (255.0 / 219.0);
    let u = (textureSample(plane_u, plane_sampler, uv).r - 128.0 / 255.0) * (255.0 / 224.0);
    let v = (textureSample(plane_v, plane_sampler, uv).r - 128.0 / 255.0) * (255.0 / 224.0);
    let rgb = clamp(vec3<f32>(
        y + 1.402 * v,
        y - 0.344136 * u - 0.714136 * v,
        y + 1.772 * u,
    ), vec3<f32>(0.0), vec3<f32>(1.0));
    let sample = vec4<f32>(rgb, 1.0);
    let premul = vec4<f32>(sample.rgb * sample.a, sample.a);
    return premul * coverage * inside.x * inside.y * scroll_coverage(in.pixel, in.scroll_clip) * in.opacity;
}

// Gamma-coded video, turned back into linear light for an sRGB target.
fn linearize_srgb(encoded_premultiplied: vec4<f32>) -> vec4<f32> {
    let alpha = encoded_premultiplied.a;
    if alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    let encoded = clamp(encoded_premultiplied.rgb / alpha, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = encoded / 12.92;
    let high = pow((encoded + 0.055) / 1.055, vec3<f32>(2.4));
    let linear = select(high, low, encoded <= vec3<f32>(0.04045));
    return vec4<f32>(linear * alpha, alpha);
}

@fragment
fn fs_video(in: VideoOutput) -> @location(0) vec4<f32> {
    return linearize_srgb(video_color(in));
}

@fragment
fn fs_video_encode_srgb(in: VideoOutput) -> @location(0) vec4<f32> {
    return video_color(in);
}
