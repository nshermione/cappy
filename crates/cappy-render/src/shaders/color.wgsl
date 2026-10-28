// Converts a linear, premultiplied color to sRGB encoding for targets without an sRGB format.
fn encode_srgb(linear_premultiplied: vec4<f32>) -> vec4<f32> {
    let alpha = linear_premultiplied.a;
    if alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    let linear = clamp(linear_premultiplied.rgb / alpha, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    let encoded = select(high, low, linear <= vec3<f32>(0.0031308));
    return vec4<f32>(encoded * alpha, alpha);
}
