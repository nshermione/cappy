//! Headless GPU rendering for pixel tests: render a [`RenderFrame`] into a texture, read it back
//! and compare it against PNG snapshots.

use std::fmt;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use cappy_core::{Color, RenderFrame};
use cappy_render::{FontContext, RenderTarget, Renderer, TargetLoad};

/// The target uses an sRGB format so read-back bytes match what a window would show.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BYTES_PER_PIXEL: u32 = 4;
/// Set to `1` to (re)write snapshots instead of comparing against them.
const UPDATE_SNAPSHOTS_ENV: &str = "CAPPY_UPDATE_SNAPSHOTS";
const ACTUAL_SUFFIX: &str = "actual.png";

#[derive(Debug)]
pub enum GpuTestError {
    NoAdapter(wgpu::RequestAdapterError),
    RequestDevice(wgpu::RequestDeviceError),
    Render(cappy_render::RenderError),
    ReadBack(String),
    Image(String),
}

impl fmt::Display for GpuTestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(error) => write!(f, "no GPU adapter for headless tests: {error}"),
            Self::RequestDevice(error) => write!(f, "failed to open the GPU device: {error}"),
            Self::Render(error) => write!(f, "rendering failed: {error}"),
            Self::ReadBack(message) => {
                write!(f, "reading the rendered image back failed: {message}")
            }
            Self::Image(message) => write!(f, "image file error: {message}"),
        }
    }
}

impl std::error::Error for GpuTestError {}

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

    pub fn save_png(&self, path: &Path) -> Result<(), GpuTestError> {
        let image_error =
            |error: &dyn fmt::Display| GpuTestError::Image(format!("{}: {error}", path.display()));
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

    pub fn load_png(path: &Path) -> Result<Self, GpuTestError> {
        let image_error =
            |error: &dyn fmt::Display| GpuTestError::Image(format!("{}: {error}", path.display()));
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

/// A GPU device without a window, plus a renderer drawing into a readable texture.
pub struct HeadlessRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

impl HeadlessRenderer {
    pub fn new(fonts: &FontContext) -> Result<Self, GpuTestError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(request_adapter(&instance))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cappy headless device"),
            ..Default::default()
        }))
        .map_err(GpuTestError::RequestDevice)?;
        let renderer = Renderer::new(&device, &queue, fonts);
        Ok(Self {
            device,
            queue,
            renderer,
        })
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Paints `frame` (only its damage, like a window would) and reads the composited result.
    pub fn render(
        &mut self,
        frame: &RenderFrame,
        background: Color,
    ) -> Result<Image, GpuTestError> {
        let (width, height) = (frame.physical_width, frame.physical_height);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cappy test target"),
            size: extent(width, height),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cappy test frame"),
            });
        self.renderer
            .render(
                &self.device,
                &self.queue,
                &mut encoder,
                frame,
                RenderTarget {
                    view: &view,
                    format: TARGET_FORMAT,
                },
                TargetLoad::Clear(background),
            )
            .map_err(GpuTestError::Render)?;
        self.read_back(encoder, &texture, width, height)
    }

    fn read_back(
        &self,
        mut encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Image, GpuTestError> {
        let unpadded_row = width * BYTES_PER_PIXEL;
        let padded_row = unpadded_row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cappy test read-back"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            extent(width, height),
        );
        let submission = self.queue.submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            // The receiver only disappears if read_back already returned an error.
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| GpuTestError::ReadBack(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| GpuTestError::ReadBack(error.to_string()))?
            .map_err(|error| GpuTestError::ReadBack(error.to_string()))?;

        let mapped = buffer
            .get_mapped_range(..)
            .map_err(|error| GpuTestError::ReadBack(error.to_string()))?;
        let pixels = mapped
            .chunks_exact(padded_row as usize)
            .flat_map(|row| &row[..unpadded_row as usize])
            .copied()
            .collect();
        Ok(Image {
            width,
            height,
            pixels,
        })
    }
}

async fn request_adapter(instance: &wgpu::Instance) -> Result<wgpu::Adapter, GpuTestError> {
    let hardware = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await;
    match hardware {
        Ok(adapter) => Ok(adapter),
        Err(_) => instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: true,
                ..Default::default()
            })
            .await
            .map_err(GpuTestError::NoAdapter),
    }
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
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
