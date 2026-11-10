//! What an image element is given: decoded pixels, or a path / URL to load.

use std::sync::Arc;

use crate::bitmap::{ImageClip, ImagePixels};

/// Pixels already in memory, or a locator the runtime loads.
///
/// A locator is a filesystem path on native (`C:\images\a.png`, `file:///...`) and a page URL in
/// the browser (`assets/a.png`, `/a.png`). `http://` and `https://` are fetched on both.
#[derive(Clone, Debug)]
pub enum ImageSource {
    Clip(ImageClip),
    Locator(String),
}

impl From<ImageClip> for ImageSource {
    fn from(clip: ImageClip) -> Self {
        Self::Clip(clip)
    }
}

impl From<ImagePixels> for ImageSource {
    fn from(pixels: ImagePixels) -> Self {
        Self::Clip(pixels.into())
    }
}

impl From<Arc<ImagePixels>> for ImageSource {
    fn from(pixels: Arc<ImagePixels>) -> Self {
        Self::Clip(pixels.into())
    }
}

impl From<&str> for ImageSource {
    fn from(locator: &str) -> Self {
        Self::Locator(locator.to_owned())
    }
}

impl From<String> for ImageSource {
    fn from(locator: String) -> Self {
        Self::Locator(locator)
    }
}

impl From<&String> for ImageSource {
    fn from(locator: &String) -> Self {
        Self::Locator(locator.clone())
    }
}

/// Locator for a file in the calling crate.
///
/// `file` is a path relative to that crate, such as `assets/photo.png`. Desktop prefixes the
/// crate directory. The browser uses `file` as the page URL. The macro expands in the caller,
/// so the directory is the application's.
///
/// ```
/// # use cappy_core::asset_path;
/// let locator = asset_path!("assets/photo.png");
/// assert!(locator.ends_with("assets/photo.png"));
/// ```
#[macro_export]
macro_rules! asset_path {
    ($file:expr) => {{
        #[cfg(target_arch = "wasm32")]
        {
            ::std::format!("{}", $file)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            ::std::format!("{}/{}", ::std::env!("CARGO_MANIFEST_DIR"), $file)
        }
    }};
}
