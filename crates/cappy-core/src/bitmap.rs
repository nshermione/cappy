//! Decoded image pixels and how they fit inside a box.

use std::fmt;
use std::sync::Arc;

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

/// Straight sRGB pixels, 4 bytes per pixel, top row first. The framework does not decode files;
/// the application hands it bytes that are already RGBA8.
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
