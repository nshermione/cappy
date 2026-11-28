//! Playlist entry points. HLS and DASH both become segment jobs.

use std::path::Path;

use cappy_core::VideoSource;

use crate::{dash, hls};

/// Opens an m3u8 playlist. Local files are read directly.
pub fn open_hls(path: impl AsRef<Path>) -> Result<VideoSource, String> {
    hls::open(path)
}

/// Opens an MPD playlist. Local files are read directly.
pub fn open_dash(path: impl AsRef<Path>) -> Result<VideoSource, String> {
    dash::open(path)
}
