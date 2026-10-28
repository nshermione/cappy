// Copies the persistent offscreen texture onto the host's target with a fullscreen triangle.

@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((vertex_index << 1u) & 2u), f32(vertex_index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

fn source_texel(position: vec4<f32>) -> vec4<f32> {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}

@fragment
fn fs_composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return source_texel(position);
}

@fragment
fn fs_composite_encode_srgb(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_srgb(source_texel(position));
}
