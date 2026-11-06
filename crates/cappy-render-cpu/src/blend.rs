//! 8-bit premultiplied-alpha pixel arithmetic, in sRGB-encoded values like tiny-skia.

/// `value * factor / 255`, rounded to nearest.
pub(crate) fn mul_div_255(value: u8, factor: u8) -> u8 {
    let product = u32::from(value) * u32::from(factor) + 128;
    ((product + (product >> 8)) >> 8) as u8
}

/// Straight RGBA to premultiplied RGBA.
pub(crate) fn premultiply([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    [
        mul_div_255(red, alpha),
        mul_div_255(green, alpha),
        mul_div_255(blue, alpha),
        alpha,
    ]
}

/// Scales every channel of a premultiplied color by `coverage` (0..=255).
pub(crate) fn scale(color: [u8; 4], coverage: u8) -> [u8; 4] {
    color.map(|channel| mul_div_255(channel, coverage))
}

/// Premultiplied `source` over premultiplied `destination`.
pub(crate) fn over(source: [u8; 4], destination: [u8; 4]) -> [u8; 4] {
    let remaining = 255 - source[3];
    std::array::from_fn(|channel| {
        source[channel].saturating_add(mul_div_255(destination[channel], remaining))
    })
}

/// Blends straight sRGB `color` over `destination` using `coverage` in linear light.
///
/// This matches glyphon's mask shader on an sRGB target: the text color is linearized, coverage
/// scales its alpha, and the blend result is encoded back to sRGB. Partial coverage therefore
/// stays lighter than an sRGB-space blend, so glyph edges stay as thin as the GPU's.
pub(crate) fn blend_linear_coverage(destination: &mut [u8], color: [u8; 4], coverage: u8) {
    if coverage == 0 || color[3] == 0 {
        return;
    }
    let src_a = (f32::from(color[3]) / 255.0) * (f32::from(coverage) / 255.0);
    let keep = 1.0 - src_a;
    let out_a = src_a + (f32::from(destination[3]) / 255.0) * keep;
    for channel in 0..3 {
        let blended =
            srgb_to_linear(color[channel]) * src_a + srgb_to_linear(destination[channel]) * keep;
        destination[channel] = linear_to_srgb(blended);
    }
    destination[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn srgb_to_linear(channel: u8) -> f32 {
    let encoded = f32::from(channel) / 255.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Blends premultiplied `source` over the RGBA pixel stored in `destination`.
pub(crate) fn blend_into(destination: &mut [u8], source: [u8; 4]) {
    if source == [0; 4] {
        return;
    }
    let current = [
        destination[0],
        destination[1],
        destination[2],
        destination[3],
    ];
    destination.copy_from_slice(&over(source, current));
}

/// Packs an opaque color as softbuffer's `0x00RRGGBB`.
pub(crate) fn to_xrgb([red, green, blue, _]: [u8; 4]) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

/// Unpacks softbuffer's `0x00RRGGBB` as an opaque color.
pub(crate) fn from_xrgb(pixel: u32) -> [u8; 4] {
    [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplication_rounds_to_nearest() {
        assert_eq!(mul_div_255(255, 255), 255);
        assert_eq!(mul_div_255(255, 0), 0);
        assert_eq!(mul_div_255(128, 128), 64);
    }

    #[test]
    fn opaque_source_replaces_the_destination() {
        assert_eq!(
            over([10, 20, 30, 255], [200, 200, 200, 255]),
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn transparent_source_keeps_the_destination() {
        assert_eq!(over([0; 4], [1, 2, 3, 4]), [1, 2, 3, 4]);
    }

    #[test]
    fn half_transparent_white_over_black() {
        let half_white = premultiply([255, 255, 255, 128]);
        assert_eq!(over(half_white, [0, 0, 0, 255]), [128, 128, 128, 255]);
    }

    #[test]
    fn half_coverage_black_on_white_stays_lighter_than_an_srgb_blend() {
        let mut pixel = [255, 255, 255, 255];
        blend_linear_coverage(&mut pixel, [0, 0, 0, 255], 128);
        assert!(
            pixel[0] > 160,
            "linear blend of half coverage is a light gray"
        );
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
        assert_eq!(pixel[3], 255);
    }

    #[test]
    fn full_coverage_replaces_the_pixel() {
        let mut pixel = [255, 255, 255, 255];
        blend_linear_coverage(&mut pixel, [10, 20, 30, 255], 255);
        assert_eq!(pixel, [10, 20, 30, 255]);
    }

    #[test]
    fn zero_coverage_leaves_the_pixel() {
        let mut pixel = [1, 2, 3, 255];
        blend_linear_coverage(&mut pixel, [0, 0, 0, 255], 0);
        assert_eq!(pixel, [1, 2, 3, 255]);
    }

    #[test]
    fn xrgb_round_trips_opaque_colors() {
        let color = [12, 34, 56, 255];
        assert_eq!(to_xrgb(color), 0x000C_2238);
        assert_eq!(from_xrgb(to_xrgb(color)), color);
    }
}
