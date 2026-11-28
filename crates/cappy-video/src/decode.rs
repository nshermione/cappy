//! Turns encoded packets into YUV pictures stored in one reused pool.

use std::sync::Arc;
use std::time::Duration;

use cappy_core::YuvPlanes;
use ffmpeg_next as ffmpeg;

use crate::packet::{EncodedPacket, StreamConfig, VideoCodec};

pub(crate) struct FrameOut {
    pub pts: Duration,
    pub key: bool,
    pub planes: YuvPlanes,
}

pub(crate) struct FfmpegDecoder {
    decoder: ffmpeg::decoder::Video,
    time_base: ffmpeg::Rational,
    pool: Pool,
    scaler: Option<ffmpeg::software::scaling::context::Context>,
    eof_sent: bool,
}

impl FfmpegDecoder {
    pub(crate) fn open(config: &StreamConfig) -> Result<Self, String> {
        let codec_id = match config.codec {
            VideoCodec::H264 => ffmpeg::codec::Id::H264,
        };
        let codec = ffmpeg::decoder::find(codec_id).ok_or_else(|| "no h264 decoder".to_owned())?;
        let mut context = ffmpeg::codec::context::Context::new_with_codec(codec);
        if !config.extradata.is_empty() {
            context.set_extradata(Some(config.extradata.as_slice()));
        }
        let decoder = context
            .decoder()
            .video()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            decoder,
            time_base: ffmpeg::Rational::new(config.time_base_num, config.time_base_den.max(1)),
            pool: Pool::default(),
            scaler: None,
            eof_sent: false,
        })
    }

    pub(crate) fn grows(&self) -> u32 {
        self.pool.grows
    }

    pub(crate) fn reset(&mut self) {
        self.decoder.flush();
        self.eof_sent = false;
    }

    pub(crate) fn push(&mut self, packet: &EncodedPacket, max_edge: u32) -> Vec<FrameOut> {
        if self.eof_sent {
            self.reset();
        }
        let mut av = ffmpeg::Packet::copy(&packet.data);
        av.set_pts(packet.pts);
        if self.decoder.send_packet(&av).is_err() {
            return Vec::new();
        }
        self.drain(max_edge)
    }

    pub(crate) fn finish(&mut self, max_edge: u32) -> Vec<FrameOut> {
        if self.eof_sent {
            return Vec::new();
        }
        self.eof_sent = true;
        let _ = self.decoder.send_eof();
        self.drain(max_edge)
    }

    fn drain(&mut self, max_edge: u32) -> Vec<FrameOut> {
        let mut frame = ffmpeg::frame::Video::empty();
        let mut out = Vec::new();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let Some(planes) = yuv420(&frame, &mut self.scaler) else {
                continue;
            };
            let (planes, width, height) = fit_edge(&planes, max_edge);
            let stored = self
                .pool
                .store(&planes.y, &planes.u, &planes.v, width, height);
            out.push(FrameOut {
                pts: media_time(frame.pts().unwrap_or(0), self.time_base),
                key: frame.is_key(),
                planes: stored,
            });
        }
        out
    }
}

#[derive(Default)]
struct Pool {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    grows: u32,
}

impl Pool {
    fn store(&mut self, y: &[u8], u: &[u8], v: &[u8], width: u32, height: u32) -> YuvPlanes {
        self.grows += grow(&mut self.y, y) + grow(&mut self.u, u) + grow(&mut self.v, v);
        self.y.clear();
        self.y.extend_from_slice(y);
        self.u.clear();
        self.u.extend_from_slice(u);
        self.v.clear();
        self.v.extend_from_slice(v);
        YuvPlanes {
            width,
            height,
            y: arc_from(&self.y),
            u: arc_from(&self.u),
            v: arc_from(&self.v),
        }
    }
}

fn grow(buf: &mut Vec<u8>, next: &[u8]) -> u32 {
    if buf.capacity() < next.len() { 1 } else { 0 }
}

fn arc_from(bytes: &[u8]) -> Arc<[u8]> {
    Arc::<[u8]>::from(bytes)
}

fn media_time(pts: i64, base: ffmpeg::Rational) -> Duration {
    let pts = pts.max(0) as f64;
    let seconds = if base.denominator() == 0 {
        0.0
    } else {
        pts * f64::from(base.numerator()) / f64::from(base.denominator())
    };
    Duration::from_secs_f64(seconds)
}

struct Planes {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    width: u32,
    height: u32,
}

fn yuv420(
    frame: &ffmpeg::frame::Video,
    scaler: &mut Option<ffmpeg::software::scaling::context::Context>,
) -> Option<Planes> {
    let width = frame.width();
    let height = frame.height();
    if frame.format() == ffmpeg::format::Pixel::YUV420P {
        return Some(copy_420(frame));
    }
    let context = scaler.get_or_insert_with(|| {
        ffmpeg::software::scaling::context::Context::get(
            frame.format(),
            width,
            height,
            ffmpeg::format::Pixel::YUV420P,
            width,
            height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )
        .expect("yuv scaler")
    });
    let mut output = ffmpeg::frame::Video::empty();
    context.run(frame, &mut output).ok()?;
    Some(copy_420(&output))
}

fn copy_420(frame: &ffmpeg::frame::Video) -> Planes {
    let width = frame.width();
    let height = frame.height();
    Planes {
        y: rows(frame.data(0), frame.stride(0), width, height),
        u: rows(
            frame.data(1),
            frame.stride(1),
            width.div_ceil(2),
            height.div_ceil(2),
        ),
        v: rows(
            frame.data(2),
            frame.stride(2),
            width.div_ceil(2),
            height.div_ceil(2),
        ),
        width,
        height,
    }
}

fn rows(data: &[u8], stride: usize, width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(width as usize * height as usize);
    let row = width as usize;
    for y in 0..height as usize {
        let start = y * stride;
        let end = (start + row).min(data.len());
        if start >= data.len() {
            out.extend(std::iter::repeat_n(0, row));
            continue;
        }
        out.extend_from_slice(&data[start..end]);
        if end - start < row {
            out.extend(std::iter::repeat_n(0, row - (end - start)));
        }
    }
    out
}

fn fit_edge(planes: &Planes, max_edge: u32) -> (Planes, u32, u32) {
    if max_edge == 0 || planes.width.max(planes.height) <= max_edge {
        return (
            Planes {
                y: planes.y.clone(),
                u: planes.u.clone(),
                v: planes.v.clone(),
                width: planes.width,
                height: planes.height,
            },
            planes.width,
            planes.height,
        );
    }
    let scale = max_edge as f32 / planes.width.max(planes.height) as f32;
    let width = ((planes.width as f32 * scale).round() as u32).max(2) & !1;
    let height = ((planes.height as f32 * scale).round() as u32).max(2) & !1;
    let scaled = scale_420(planes, width, height);
    (scaled, width, height)
}

pub(crate) fn fit_planes(planes: &YuvPlanes, max_edge: u32) -> YuvPlanes {
    let source = Planes {
        y: planes.y.to_vec(),
        u: planes.u.to_vec(),
        v: planes.v.to_vec(),
        width: planes.width,
        height: planes.height,
    };
    let (scaled, width, height) = fit_edge(&source, max_edge);
    YuvPlanes {
        width,
        height,
        y: arc_from(&scaled.y),
        u: arc_from(&scaled.u),
        v: arc_from(&scaled.v),
    }
}

fn scale_420(planes: &Planes, width: u32, height: u32) -> Planes {
    let chroma_w = width.div_ceil(2);
    let chroma_h = height.div_ceil(2);
    Planes {
        y: scale_plane(&planes.y, planes.width, planes.height, width, height),
        u: scale_plane(
            &planes.u,
            planes.width.div_ceil(2),
            planes.height.div_ceil(2),
            chroma_w,
            chroma_h,
        ),
        v: scale_plane(
            &planes.v,
            planes.width.div_ceil(2),
            planes.height.div_ceil(2),
            chroma_w,
            chroma_h,
        ),
        width,
        height,
    }
}

fn scale_plane(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut out = vec![0; dst_w as usize * dst_h as usize];
    for y in 0..dst_h {
        let sy = (y as u64 * src_h as u64 / dst_h as u64) as u32;
        for x in 0..dst_w {
            let sx = (x as u64 * src_w as u64 / dst_w as u64) as u32;
            let index = (sy as usize * src_w as usize) + sx as usize;
            out[(y as usize * dst_w as usize) + x as usize] = src.get(index).copied().unwrap_or(0);
        }
    }
    out
}
