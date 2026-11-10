//! Decode browser image files into [`ImagePixels`](crate::ImagePixels).

use std::fmt;

use std::sync::Arc;

use crate::bitmap::{ImageClip, ImagePixels, ImagePixelsError};

/// Why [`ImagePixels::decode`](crate::ImagePixels::decode) rejected a buffer.
#[derive(Debug, PartialEq, Eq)]
pub enum ImageDecodeError {
    /// `bytes` is empty.
    Empty,
    /// The bytes are not a PNG, JPEG, GIF, WebP, BMP, ICO, SVG, or AVIF image.
    Unrecognized,
    /// The format was recognized, but the bytes did not become a bitmap.
    ///
    /// This includes a decoder error and AVIF on a build that has no AVIF decoder.
    Failed(String),
}

impl fmt::Display for ImageDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "image is empty"),
            Self::Unrecognized => write!(formatter, "unrecognized image format"),
            Self::Failed(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ImageDecodeError {}

pub(crate) fn decode_image(bytes: &[u8]) -> Result<ImagePixels, ImageDecodeError> {
    if bytes.is_empty() {
        return Err(ImageDecodeError::Empty);
    }
    // dav1d-sys needs a system dav1d (pkg-config) or meson, ninja, and nasm. This
    // workspace has none of those, so AVIF is recognized and rejected instead of
    // being misread as another format or failing the build.
    if is_avif(bytes) {
        return Err(ImageDecodeError::Failed(
            "AVIF decoding is not compiled for this target".to_owned(),
        ));
    }
    if looks_like_svg(bytes) {
        return decode_svg(bytes);
    }
    decode_raster(bytes)
}

pub(crate) fn decode_clip(bytes: &[u8]) -> Result<ImageClip, ImageDecodeError> {
    if is_gif(bytes) {
        return decode_gif(bytes);
    }
    Ok(ImageClip::still(decode_image(bytes)?))
}

fn decode_gif(bytes: &[u8]) -> Result<ImageClip, ImageDecodeError> {
    use image::AnimationDecoder;

    let decoder =
        image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).map_err(failed)?;
    let loops = match decoder.loop_count() {
        image::metadata::LoopCount::Infinite => None,
        image::metadata::LoopCount::Finite(count) => Some(count.get()),
    };
    let decoded = decoder.into_frames().collect_frames().map_err(failed)?;
    if decoded.is_empty() {
        return Err(ImageDecodeError::Failed(
            "failed to decode image: gif has no frames".to_owned(),
        ));
    }
    let mut frames = Vec::with_capacity(decoded.len());
    let mut delays_ms = Vec::with_capacity(decoded.len());
    for frame in decoded {
        let (numer, denom) = frame.delay().numer_denom_ms();
        delays_ms.push(gif_delay_ms(numer, denom));
        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        let pixels =
            ImagePixels::from_rgba(width, height, buffer.into_raw()).map_err(pixels_error)?;
        frames.push(Arc::new(pixels));
    }
    Ok(ImageClip::from_parts(frames, delays_ms, loops))
}

/// Chrome treats a GIF delay under 20ms as 100ms. A zero delay would otherwise freeze the loop.
fn gif_delay_ms(numer: u32, denom: u32) -> u32 {
    let ms = if denom == 0 { 0 } else { numer / denom };
    if ms < 20 { 100 } else { ms }
}

fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

fn decode_raster(bytes: &[u8]) -> Result<ImagePixels, ImageDecodeError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(failed)?;
    if reader.format().is_none() {
        return Err(ImageDecodeError::Unrecognized);
    }
    let decoded = reader.decode().map_err(failed)?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    ImagePixels::from_rgba(width, height, rgba.into_raw()).map_err(pixels_error)
}

fn decode_svg(bytes: &[u8]) -> Result<ImagePixels, ImageDecodeError> {
    let options = resvg::usvg::Options {
        default_size: resvg::usvg::Size::from_wh(512.0, 512.0)
            .expect("512 is a positive svg default size"),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(failed)?;
    let svg_size = tree.size();
    let (width, height) = if svg_has_intrinsic_size(bytes) {
        (raster_px(svg_size.width()), raster_px(svg_size.height()))
    } else {
        fit_long_edge(svg_size.width(), svg_size.height(), 512.0)
    };
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        ImageDecodeError::Failed("failed to decode image: svg raster is too large".to_owned())
    })?;
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / svg_size.width(),
        height as f32 / svg_size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    ImagePixels::from_rgba(width, height, straight_rgba(pixmap.data())).map_err(pixels_error)
}

fn failed(error: impl fmt::Display) -> ImageDecodeError {
    ImageDecodeError::Failed(format!("failed to decode image: {error}"))
}

fn pixels_error(error: ImagePixelsError) -> ImageDecodeError {
    ImageDecodeError::Failed(format!("failed to decode image: {error}"))
}

/// `width`, `height`, or `viewBox` on the root `<svg>` element. Without one of those,
/// resvg sizes the tree from its content, and the caller scales the long edge to 512.
fn svg_has_intrinsic_size(bytes: &[u8]) -> bool {
    let Some(tag) = opening_svg_tag(skip_ascii_whitespace(without_bom(bytes))) else {
        return false;
    };
    has_svg_attr(tag, b"width") || has_svg_attr(tag, b"height") || has_svg_attr(tag, b"viewbox")
}

fn opening_svg_tag(mut bytes: &[u8]) -> Option<&[u8]> {
    loop {
        bytes = skip_ascii_whitespace(bytes);
        if bytes.starts_with(b"<?") {
            bytes = skip_until(bytes, b"?>")?;
            continue;
        }
        if bytes.starts_with(b"<!--") {
            bytes = skip_until(bytes, b"-->")?;
            continue;
        }
        if starts_with_ignore_ascii_case(bytes, b"<!doctype") {
            bytes = skip_until(bytes, b">")?;
            continue;
        }
        break;
    }
    if !starts_with_ignore_ascii_case(bytes, b"<svg") {
        return None;
    }
    let mut quote = None;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        match (quote, byte) {
            (None, b'"' | b'\'') => quote = Some(*byte),
            (Some(open), byte) if *byte == open => quote = None,
            (None, b'>') => return Some(&bytes[..=index]),
            _ => {}
        }
    }
    None
}

fn skip_until<'a>(bytes: &'a [u8], marker: &[u8]) -> Option<&'a [u8]> {
    let index = bytes
        .windows(marker.len())
        .position(|window| window == marker)?;
    Some(&bytes[index + marker.len()..])
}

fn has_svg_attr(tag: &[u8], name: &[u8]) -> bool {
    let lower: Vec<u8> = tag.iter().map(u8::to_ascii_lowercase).collect();
    let mut search_from = 0;
    while let Some(found) = lower[search_from..]
        .windows(name.len())
        .position(|window| window == name)
    {
        let index = search_from + found;
        let before = index == 0 || lower[index - 1].is_ascii_whitespace();
        let after = index + name.len();
        let equals = lower[after..]
            .iter()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| *byte == b'=');
        if before && equals {
            return true;
        }
        search_from = index + name.len();
    }
    false
}

fn fit_long_edge(width: f32, height: f32, long_edge: f32) -> (u32, u32) {
    let longest = width.max(height);
    if !longest.is_finite() || longest <= 0.0 {
        let edge = raster_px(long_edge);
        return (edge, edge);
    }
    let scale = long_edge / longest;
    (raster_px(width * scale), raster_px(height * scale))
}

fn raster_px(value: f32) -> u32 {
    let rounded = value.round();
    if !rounded.is_finite() || rounded < 1.0 {
        1
    } else if rounded >= u32::MAX as f32 {
        u32::MAX
    } else {
        rounded as u32
    }
}

/// ISO BMFF files whose `ftyp` brands include AVIF. The minor version sits between the
/// major brand and the compatible brands, so a 4-byte step can land on it; that word is
/// not `avif` or `avis`.
fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let declared = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let end = declared.clamp(12, bytes.len());
    let mut offset = 8;
    while offset + 4 <= end {
        let brand = &bytes[offset..offset + 4];
        if brand == b"avif" || brand == b"avis" {
            return true;
        }
        offset += 4;
    }
    false
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let bytes = skip_ascii_whitespace(without_bom(bytes));
    starts_with_ignore_ascii_case(bytes, b"<svg")
        || starts_with_ignore_ascii_case(bytes, b"<?xml")
        || starts_with_ignore_ascii_case(bytes, b"<!doctype")
        || starts_with_ignore_ascii_case(bytes, b"<!--")
}

fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

fn skip_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

fn starts_with_ignore_ascii_case(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn straight_rgba(premultiplied: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(premultiplied.len());
    for pixel in premultiplied.as_chunks::<4>().0 {
        let alpha = pixel[3];
        if alpha == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else if alpha == 255 {
            rgba.extend_from_slice(pixel);
        } else {
            rgba.push(unpremultiply(pixel[0], alpha));
            rgba.push(unpremultiply(pixel[1], alpha));
            rgba.push(unpremultiply(pixel[2], alpha));
            rgba.push(alpha);
        }
    }
    rgba
}

fn unpremultiply(channel: u8, alpha: u8) -> u8 {
    let value = (u16::from(channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha);
    u8::try_from(value).unwrap_or(255)
}

#[cfg(test)]
mod tests {
    use super::ImageDecodeError;
    use crate::ImagePixels;

    fn decode(bytes: &[u8]) -> ImagePixels {
        ImagePixels::decode(bytes).expect("fixture decodes")
    }

    fn pixel(pixels: &ImagePixels, x: u32, y: u32) -> &[u8] {
        let index = ((y * pixels.width() + x) * 4) as usize;
        &pixels.rgba()[index..index + 4]
    }

    #[test]
    fn png_keeps_straight_alpha() {
        let pixels = decode(include_bytes!("../fixtures/sample.png"));
        assert_eq!((pixels.width(), pixels.height()), (32, 32));
        assert_eq!(pixel(&pixels, 0, 0), &[255, 0, 0, 128]);
        assert_eq!(pixel(&pixels, 16, 16), &[0, 0, 255, 255]);
    }

    #[test]
    fn jpeg_uses_the_file_size() {
        let pixels = decode(include_bytes!("../fixtures/sample.jpg"));
        assert_eq!((pixels.width(), pixels.height()), (40, 24));
        assert_eq!(pixels.rgba().len(), 40 * 24 * 4);
    }

    #[test]
    fn gif_clip_keeps_both_frames() {
        use std::time::Duration;

        use crate::ImageClip;

        let clip = ImageClip::decode(include_bytes!("../fixtures/sample.gif")).expect("gif");
        assert!(clip.is_animated());
        assert_eq!(clip.index_at(Duration::ZERO), 0);
        assert_eq!(clip.index_at(Duration::from_millis(100)), 1);
        assert_eq!(&clip.frame_pixels(0).rgba()[..4], &[255, 0, 0, 255]);
        assert_eq!(&clip.frame_pixels(1).rgba()[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn gif_decodes_the_first_frame_only() {
        let pixels = decode(include_bytes!("../fixtures/sample.gif"));
        assert_eq!((pixels.width(), pixels.height()), (32, 32));
        assert_eq!(pixel(&pixels, 0, 0), &[255, 0, 0, 255]);
        assert_ne!(pixel(&pixels, 0, 0), &[0, 0, 255, 255]);
    }

    #[test]
    fn webp_bmp_and_ico_decode() {
        let webp = decode(include_bytes!("../fixtures/sample.webp"));
        assert_eq!((webp.width(), webp.height()), (36, 36));
        assert_eq!(pixel(&webp, 0, 0), &[249, 115, 22, 255]);

        let bmp = decode(include_bytes!("../fixtures/sample.bmp"));
        assert_eq!((bmp.width(), bmp.height()), (28, 20));
        assert_eq!(pixel(&bmp, 0, 0), &[168, 85, 247, 255]);

        let ico = decode(include_bytes!("../fixtures/sample.ico"));
        assert_eq!((ico.width(), ico.height()), (16, 16));
        assert_eq!(pixel(&ico, 0, 0), &[6, 182, 212, 255]);
    }

    #[test]
    fn svg_uses_intrinsic_width_and_height() {
        let pixels = decode(include_bytes!("../fixtures/sample.svg"));
        assert_eq!((pixels.width(), pixels.height()), (64, 48));
        assert_eq!(pixel(&pixels, 1, 1), &[56, 189, 248, 255]);
    }

    #[test]
    fn svg_view_box_sets_the_size_when_width_and_height_are_absent() {
        let pixels = decode(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10"><rect width="20" height="10" fill="black"/></svg>"#,
        );
        assert_eq!((pixels.width(), pixels.height()), (20, 10));
    }

    #[test]
    fn svg_without_a_size_uses_a_512_long_edge() {
        let square = decode(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" fill="black"/></svg>"#,
        );
        assert_eq!((square.width(), square.height()), (512, 512));

        let wide = decode(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="20" height="10" fill="black"/></svg>"#,
        );
        assert_eq!((wide.width(), wide.height()), (512, 256));
    }

    #[test]
    fn svg_allows_a_bom_whitespace_and_an_xml_declaration() {
        let mut bom = vec![0xEF, 0xBB, 0xBF, b'\n', b' '];
        bom.extend_from_slice(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"></svg>"#,
        );
        let pixels = decode(&bom);
        assert_eq!((pixels.width(), pixels.height()), (8, 4));

        let declared = decode(
            br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="3" height="5"></svg>"#,
        );
        assert_eq!((declared.width(), declared.height()), (3, 5));
    }

    #[test]
    fn svg_straightens_premultiplied_alpha() {
        let pixels = decode(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"><rect width="1" height="1" fill="red"/><rect x="1" width="1" height="1" fill="blue" fill-opacity="0.5"/></svg>"#,
        );
        assert_eq!(pixel(&pixels, 0, 0), &[255, 0, 0, 255]);
        let translucent = pixel(&pixels, 1, 0);
        assert!(translucent[2] > 200, "blue was {translucent:?}");
        assert!(
            (100..=160).contains(&translucent[3]),
            "alpha was {translucent:?}"
        );
    }

    #[test]
    fn empty_unknown_and_truncated_bytes_fail() {
        assert_eq!(ImagePixels::decode(b""), Err(ImageDecodeError::Empty));
        assert_eq!(
            ImagePixels::decode(b"hello"),
            Err(ImageDecodeError::Unrecognized)
        );
        assert!(matches!(
            ImagePixels::decode(b"\x89PNG\r\n\x1a\n"),
            Err(ImageDecodeError::Failed(_))
        ));
    }

    #[test]
    fn avif_is_rejected_without_being_treated_as_an_unknown_format() {
        let mut avif = vec![0, 0, 0, 20];
        avif.extend_from_slice(b"ftypavif");
        avif.extend_from_slice(&[0, 0, 0, 0]);
        avif.extend_from_slice(b"mif1");
        match ImagePixels::decode(&avif) {
            Err(ImageDecodeError::Failed(message)) => {
                assert!(message.contains("AVIF"), "{message}")
            }
            other => panic!("expected an AVIF failure, got {other:?}"),
        }

        let mut heic = vec![0, 0, 0, 12];
        heic.extend_from_slice(b"ftypheic");
        assert_eq!(
            ImagePixels::decode(&heic),
            Err(ImageDecodeError::Unrecognized)
        );
    }
}
