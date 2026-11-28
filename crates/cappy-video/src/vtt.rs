//! WebVTT cues, plus subtitle streams read from a container.

use std::path::Path;
use std::time::Duration;

use cappy_core::{TextCue, TextTrack};
use ffmpeg_next as ffmpeg;

pub(crate) struct TextList {
    pub tracks: Vec<TextTrack>,
    pub cues: Vec<(u32, Vec<TextCue>)>,
}

pub(crate) fn parse_vtt(text: &str) -> Vec<TextCue> {
    let mut cues = Vec::new();
    let mut lines = text.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        let Some((start, end)) = line.split_once("-->") else {
            continue;
        };
        let Some(start) = parse_stamp(start.trim()) else {
            continue;
        };
        let Some(end) = parse_stamp(end.split_whitespace().next().unwrap_or("")) else {
            continue;
        };
        let mut body = Vec::new();
        while let Some(next) = lines.peek() {
            if next.is_empty() || next.contains("-->") {
                break;
            }
            body.push(lines.next().unwrap_or(""));
        }
        let text = body.join("\n").trim().to_owned();
        if text.is_empty() {
            continue;
        }
        cues.push(TextCue { start, end, text });
    }
    cues
}

pub(crate) fn load_vtt(path: &Path) -> Vec<TextCue> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    if text.trim_start().starts_with("WEBVTT") || text.contains("-->") {
        return parse_vtt(&text);
    }
    Vec::new()
}

/// Subtitle streams in a container ffmpeg can open. Packets are decoded to text and then dropped.
pub(crate) fn from_container(path: &Path) -> TextList {
    let Ok(input) = ffmpeg::format::input(path) else {
        return TextList {
            tracks: Vec::new(),
            cues: Vec::new(),
        };
    };
    let streams: Vec<_> = input
        .streams()
        .filter(|stream| stream.parameters().medium() == ffmpeg::media::Type::Subtitle)
        .map(|stream| {
            let id = stream.index() as u32;
            let language = stream
                .metadata()
                .get("language")
                .unwrap_or("und")
                .to_owned();
            let label = stream
                .metadata()
                .get("title")
                .unwrap_or(&language)
                .to_owned();
            (
                id,
                language,
                label,
                stream.time_base(),
                stream.parameters().clone(),
            )
        })
        .collect();
    if streams.is_empty() {
        return TextList {
            tracks: Vec::new(),
            cues: Vec::new(),
        };
    }
    let mut tracks = Vec::new();
    let mut cues = Vec::new();
    for (id, language, label, time_base, parameters) in &streams {
        tracks.push(TextTrack {
            id: *id,
            language: language.clone(),
            label: label.clone(),
        });
        let Ok(context) = ffmpeg::codec::context::Context::from_parameters(parameters.clone())
        else {
            cues.push((*id, Vec::new()));
            continue;
        };
        let Ok(mut decoder) = context.decoder().subtitle() else {
            cues.push((*id, Vec::new()));
            continue;
        };
        let mut decoded = Vec::new();
        let Ok(mut input) = ffmpeg::format::input(path) else {
            cues.push((*id, Vec::new()));
            continue;
        };
        for (stream, packet) in input.packets() {
            if stream.index() as u32 != *id {
                continue;
            }
            let mut subtitle = ffmpeg::Subtitle::new();
            if decoder.decode(&packet, &mut subtitle).ok() != Some(true) {
                continue;
            }
            let text = subtitle
                .rects()
                .filter_map(|rect| match rect {
                    ffmpeg::codec::subtitle::Rect::Text(text) => Some(plain(text.get())),
                    ffmpeg::codec::subtitle::Rect::Ass(text) => Some(plain(text.get())),
                    _ => None,
                })
                .find(|text| !text.is_empty());
            let Some(text) = text else {
                continue;
            };
            let start = ticks(packet.pts().unwrap_or(0), *time_base)
                + Duration::from_millis(u64::from(subtitle.start()));
            let mut end = ticks(packet.pts().unwrap_or(0), *time_base)
                + Duration::from_millis(u64::from(subtitle.end()));
            if end <= start {
                end = start + ticks(packet.duration(), *time_base);
            }
            if end <= start {
                end = start + Duration::from_secs(2);
            }
            decoded.push(TextCue { start, end, text });
        }
        cues.push((*id, decoded));
    }
    TextList { tracks, cues }
}

fn plain(raw: &str) -> String {
    let line = raw.lines().next().unwrap_or(raw).trim();
    if let Some(rest) = line.strip_prefix("Dialogue:") {
        return rest.rsplit(',').next().unwrap_or(rest).trim().to_owned();
    }
    line.to_owned()
}

fn ticks(value: i64, time_base: ffmpeg::Rational) -> Duration {
    if time_base.denominator() == 0 {
        return Duration::ZERO;
    }
    let seconds =
        value.max(0) as f64 * f64::from(time_base.numerator()) / f64::from(time_base.denominator());
    Duration::from_secs_f64(seconds)
}

fn parse_stamp(text: &str) -> Option<Duration> {
    let (body, millis) = text.split_once('.').unwrap_or((text, "0"));
    let parts: Vec<u64> = body
        .split(':')
        .filter_map(|part| part.parse().ok())
        .collect();
    let (hours, minutes, seconds) = match *parts.as_slice() {
        [minutes, seconds] => (0, minutes, seconds),
        [hours, minutes, seconds] => (hours, minutes, seconds),
        _ => return None,
    };
    let millis: u64 = millis.chars().take(3).collect::<String>().parse().ok()?;
    Some(Duration::from_millis(
        hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis,
    ))
}
