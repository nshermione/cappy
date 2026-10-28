//! RGBA images read back from a renderer, pixel probes and PNG snapshots shared by the GPU and
//! CPU pixel tests.

use std::fmt;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

const BYTES_PER_PIXEL: u32 = 4;
/// Set to `1` to (re)write snapshots instead of comparing against them.
const UPDATE_SNAPSHOTS_ENV: &str = "CAPPY_UPDATE_SNAPSHOTS";
const ACTUAL_SUFFIX: &str = "actual.png";

/// Reading or writing an image file failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageError(pub String);

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "image file error: {}", self.0)
    }
}

impl std::error::Error for ImageError {}

/// An RGBA8 (sRGB) image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) is outside the image"
        );
        let start = ((y * self.width + x) * BYTES_PER_PIXEL) as usize;
        let mut pixel = [0; 4];
        pixel.copy_from_slice(&self.pixels[start..start + 4]);
        pixel
    }

    /// Number of pixels where any channel differs by more than `tolerance`.
    pub fn count_differences(&self, other: &Image, tolerance: u8) -> usize {
        assert_eq!(
            (self.width, self.height),
            (other.width, other.height),
            "images differ in size"
        );
        self.pixels
            .chunks_exact(BYTES_PER_PIXEL as usize)
            .zip(other.pixels.chunks_exact(BYTES_PER_PIXEL as usize))
            .filter(|(left, right)| {
                left.iter()
                    .zip(right.iter())
                    .any(|(a, b)| a.abs_diff(*b) > tolerance)
            })
            .count()
    }

    pub fn save_png(&self, path: &Path) -> Result<(), ImageError> {
        let image_error =
            |error: &dyn fmt::Display| ImageError(format!("{}: {error}", path.display()));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| image_error(&error))?;
        }
        let file = File::create(path).map_err(|error| image_error(&error))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| image_error(&error))?;
        writer
            .write_image_data(&self.pixels)
            .map_err(|error| image_error(&error))
    }

    pub fn load_png(path: &Path) -> Result<Self, ImageError> {
        let image_error =
            |error: &dyn fmt::Display| ImageError(format!("{}: {error}", path.display()));
        let file = File::open(path).map_err(|error| image_error(&error))?;
        let mut reader = png::Decoder::new(BufReader::new(file))
            .read_info()
            .map_err(|error| image_error(&error))?;
        let buffer_size = reader
            .output_buffer_size()
            .ok_or_else(|| image_error(&"image too large"))?;
        let mut pixels = vec![0; buffer_size];
        let info = reader
            .next_frame(&mut pixels)
            .map_err(|error| image_error(&error))?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(image_error(&"snapshots must be 8-bit RGBA"));
        }
        pixels.truncate(info.buffer_size());
        Ok(Self {
            width: info.width,
            height: info.height,
            pixels,
        })
    }
}

/// Compares `image` with `<directory>/<name>.png`. A missing snapshot is written; with
/// `CAPPY_UPDATE_SNAPSHOTS=1` every snapshot is rewritten. On mismatch the actual image is
/// saved next to the snapshot and the test panics.
pub fn assert_snapshot(directory: &Path, name: &str, image: &Image, tolerance: u8) {
    let expected_path = directory.join(format!("{name}.png"));
    let update = std::env::var(UPDATE_SNAPSHOTS_ENV).is_ok_and(|value| value == "1");
    if update || !expected_path.exists() {
        image
            .save_png(&expected_path)
            .unwrap_or_else(|error| panic!("cannot write snapshot: {error}"));
        return;
    }
    let expected = Image::load_png(&expected_path)
        .unwrap_or_else(|error| panic!("cannot read snapshot: {error}"));
    let same_size = (expected.width, expected.height) == (image.width, image.height);
    let differences = if same_size {
        expected.count_differences(image, tolerance)
    } else {
        usize::MAX
    };
    if differences > 0 {
        let actual_path: PathBuf = directory.join(format!("{name}.{ACTUAL_SUFFIX}"));
        image
            .save_png(&actual_path)
            .unwrap_or_else(|error| panic!("cannot write actual image: {error}"));
        panic!(
            "snapshot {name} differs in {differences} pixels; see {}",
            actual_path.display()
        );
    }
}
