//! Decoded image pixels and how they fit inside a box.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

pub use crate::image_decode::ImageDecodeError;

/// How an image fills the box it is laid out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ObjectFit {
    /// Stretch the image to the box, ignoring its aspect ratio.
    #[default]
    Fill,
    /// Fit the whole image inside the box, leaving empty bands on the longer axis.
    Contain,
    /// Fill the box and crop the overflow, keeping the image centered.
    Cover,
}

/// Why [`ImagePixels::from_rgba`] rejected a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImagePixelsError {
    Empty,
    Length { expected: usize, actual: usize },
}

impl fmt::Display for ImagePixelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "an image needs a non-zero width and height"),
            Self::Length { expected, actual } => {
                write!(f, "expected {expected} RGBA bytes, found {actual}")
            }
        }
    }
}

impl std::error::Error for ImagePixelsError {}

/// Straight sRGB pixels, 4 bytes per pixel, top row first.
///
/// Build one from RGBA with [`from_rgba`](Self::from_rgba), or decode a file with
/// [`decode`](Self::decode).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePixels {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

impl ImagePixels {
    pub fn from_rgba(
        width: u32,
        height: u32,
        rgba: impl Into<Arc<[u8]>>,
    ) -> Result<Self, ImagePixelsError> {
        if width == 0 || height == 0 {
            return Err(ImagePixelsError::Empty);
        }
        let rgba = rgba.into();
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(ImagePixelsError::Length {
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Decodes a browser image into straight RGBA8.
    ///
    /// Accepts PNG, JPEG, GIF, WebP, BMP, ICO, and SVG. AVIF is recognized, then
    /// rejected with [`ImageDecodeError::Failed`]: the dav1d decoder is not linked,
    /// because building it needs pkg-config or meson. A GIF yields its first frame;
    /// [`ImageClip::decode`] keeps every frame so an image element can play it.
    /// APNG and animated WebP still yield the first frame. SVG shapes are rasterized
    /// once at the intrinsic size (`width`/`height`, or the `viewBox` when those are
    /// absent). An SVG with neither uses a 512px long edge. Text and embedded bitmaps
    /// inside an SVG are skipped. Resizing the element later scales that bitmap; it
    /// does not rasterize again.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageDecodeError> {
        crate::image_decode::decode_image(bytes)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// One or more frames of an image. A GIF with more than one frame plays; everything else is still.
#[derive(Clone, Debug)]
pub struct ImageClip {
    frames: Arc<[Arc<ImagePixels>]>,
    delays_ms: Arc<[u32]>,
    /// `None` repeats forever. `Some(n)` plays the whole sequence `n` times, then holds the last frame.
    loops: Option<u32>,
}

impl ImageClip {
    pub fn still(pixels: impl Into<Arc<ImagePixels>>) -> Self {
        Self {
            frames: Arc::from([pixels.into()]),
            delays_ms: Arc::from([0]),
            loops: Some(1),
        }
    }

    /// Decodes the same formats as [`ImagePixels::decode`]. A GIF keeps every frame and its delay.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageDecodeError> {
        crate::image_decode::decode_clip(bytes)
    }

    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    pub fn frame_pixels(&self, index: usize) -> Arc<ImagePixels> {
        let last = self.frames.len() - 1;
        self.frames[index.min(last)].clone()
    }

    /// Frame visible after `elapsed`. A finished finite loop stays on the last frame.
    pub fn index_at(&self, elapsed: Duration) -> usize {
        self.place(elapsed)
            .map(|(index, _)| index)
            .unwrap_or(self.frames.len() - 1)
    }

    /// Time until the shown frame changes. `None` when the clip is still or has finished.
    pub fn remaining(&self, elapsed: Duration) -> Option<Duration> {
        let (_, left_ms) = self.place(elapsed)?;
        Some(Duration::from_millis(left_ms.max(1)))
    }

    pub(crate) fn from_parts(
        frames: Vec<Arc<ImagePixels>>,
        delays_ms: Vec<u32>,
        loops: Option<u32>,
    ) -> Self {
        Self {
            frames: frames.into(),
            delays_ms: delays_ms.into(),
            loops,
        }
    }

    /// `(frame, milliseconds left in that frame)`. `None` when a finite loop has ended.
    fn place(&self, elapsed: Duration) -> Option<(usize, u64)> {
        if !self.is_animated() {
            return None;
        }
        let cycle = self
            .delays_ms
            .iter()
            .map(|delay| u128::from(*delay))
            .sum::<u128>();
        if cycle == 0 {
            return None;
        }
        let elapsed_ms = elapsed.as_millis();
        if let Some(limit) = self.loops
            && elapsed_ms / cycle >= u128::from(limit)
        {
            return None;
        }
        let mut into = elapsed_ms % cycle;
        for (index, delay) in self.delays_ms.iter().enumerate() {
            let delay = u128::from(*delay);
            if into < delay {
                return Some((index, (delay - into) as u64));
            }
            into -= delay;
        }
        Some((self.frames.len() - 1, 1))
    }
}

impl From<ImagePixels> for ImageClip {
    fn from(pixels: ImagePixels) -> Self {
        Self::still(pixels)
    }
}

impl From<Arc<ImagePixels>> for ImageClip {
    fn from(pixels: Arc<ImagePixels>) -> Self {
        Self::still(pixels)
    }
}
