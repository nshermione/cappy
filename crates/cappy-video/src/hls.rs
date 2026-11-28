//! An m3u8 adapter. It turns a playlist into segment jobs and nothing else.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cappy_core::{AudioTrack, VideoQuality};

use crate::file::launch;
use crate::locator::{read_text, resolve};
use crate::segment::{NextSegment, Pipeline, SegmentJob, SegmentSource};
use crate::session::Session;

struct MediaSegment {
    path: PathBuf,
    start: Duration,
    duration: Duration,
    key: Option<crate::segment::SegmentKey>,
}

struct AudioRendition {
    language: String,
    segments: Vec<String>,
}

struct Variant {
    edge: u32,
    label: String,
    bandwidth: u64,
    segments: Vec<MediaSegment>,
    end_list: bool,
}

/// Longest edge of the first segment. 184p fits; 288p and above wait on a slow link.
const STARTUP_LONG_EDGE: u32 = 360;

pub(crate) struct HlsPlaylist {
    path: PathBuf,
    variants: Vec<Variant>,
    current: usize,
    cursor: usize,
    delay: Duration,
    edge: Duration,
    started: bool,
    session: Option<Arc<Session>>,
}

impl HlsPlaylist {
    fn media(path: &Path) -> Result<Self, String> {
        let text = read_text(path)?;
        let (segments, end_list) = parse_media(&text, path)?;
        let edge = segments
            .last()
            .map(|segment| segment.start + segment.duration)
            .unwrap_or_default();
        Ok(Self {
            path: path.to_path_buf(),
            variants: vec![Variant {
                edge: 0,
                label: String::new(),
                bandwidth: 0,
                segments,
                end_list,
            }],
            current: 0,
            cursor: 0,
            delay: Duration::ZERO,
            edge,
            started: false,
            session: None,
        })
    }

    fn with_variants(path: &Path, variants: Vec<Variant>, delay: Duration) -> Result<Self, String> {
        let edge = variants
            .first()
            .and_then(|variant| variant.segments.last())
            .map(|segment| segment.start + segment.duration)
            .unwrap_or_default();
        Ok(Self {
            path: path.to_path_buf(),
            variants,
            current: 0,
            cursor: 0,
            delay,
            edge,
            started: false,
            session: None,
        })
    }

    fn attach(&mut self, session: Arc<Session>) {
        self.session = Some(session);
    }

    fn reload(&mut self) {
        if self.variants.len() != 1 {
            return;
        }
        let Ok(fresh) = Self::media(&self.path) else {
            return;
        };
        let Some(variant) = fresh.variants.into_iter().next() else {
            return;
        };
        if variant.segments.len() >= self.variants[0].segments.len() {
            self.edge = fresh.edge;
            self.variants[0] = variant;
            if let Some(session) = &self.session {
                session.set_duration(self.edge);
            }
        }
    }

    fn active(&self) -> &Variant {
        &self.variants[self.current]
    }

    fn choose_initial(&mut self) {
        if self.pinned() {
            self.apply_pin();
            return;
        }
        let max_edge = self
            .session
            .as_ref()
            .map(|session| session.max_edge())
            .unwrap_or(0);
        self.current = self.startup(max_edge);
    }

    fn pinned(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|session| session.pinned_quality())
            .is_some()
    }

    fn apply_pin(&mut self) {
        let Some(edge) = self
            .session
            .as_ref()
            .and_then(|session| session.pinned_quality())
        else {
            return;
        };
        if let Some(index) = self.best(edge) {
            self.current = index;
        }
    }

    /// A small first segment, so the picture shows before a large rung has downloaded.
    fn startup(&self, max_edge: u32) -> usize {
        let eligible: Vec<usize> = self
            .variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| max_edge == 0 || variant.edge == 0 || variant.edge <= max_edge)
            .map(|(index, _)| index)
            .collect();
        let modest = eligible.iter().copied().find(|&index| {
            let edge = self.variants[index].edge;
            edge > 0 && edge <= STARTUP_LONG_EDGE
        });
        modest.or_else(|| eligible.last().copied()).unwrap_or(0)
    }

    fn step_down(&mut self) -> bool {
        if self.pinned() {
            return false;
        }
        let max_edge = self
            .session
            .as_ref()
            .map(|session| session.max_edge())
            .unwrap_or(0);
        let eligible: Vec<usize> = self
            .variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| max_edge == 0 || variant.edge == 0 || variant.edge <= max_edge)
            .map(|(index, _)| index)
            .collect();
        let Some(place) = eligible.iter().position(|index| *index == self.current) else {
            return false;
        };
        let Some(next) = eligible.get(place + 1) else {
            return false;
        };
        self.current = *next;
        true
    }

    fn best(&self, max_edge: u32) -> Option<usize> {
        self.variants
            .iter()
            .enumerate()
            .find_map(|(index, variant)| {
                let fits = max_edge == 0 || variant.edge == 0 || variant.edge <= max_edge;
                fits.then_some(index)
            })
    }
}

impl SegmentSource for HlsPlaylist {
    fn next_segment(&mut self) -> NextSegment {
        if !self.started {
            self.choose_initial();
            self.started = true;
        } else {
            self.apply_pin();
        }
        let end_list = self.active().end_list;
        if self.cursor >= self.active().segments.len() && !end_list {
            self.reload();
        }
        let Some(segment) = self.active().segments.get(self.cursor) else {
            return if self.active().end_list {
                NextSegment::Done
            } else {
                NextSegment::Wait
            };
        };
        let job = SegmentJob {
            path: segment.path.clone(),
            start: segment.start,
            duration: segment.duration,
            key: segment.key.clone(),
            delay: self.delay,
            init: None,
        };
        self.cursor += 1;
        NextSegment::Job(job)
    }

    fn upcoming(&self) -> Option<PathBuf> {
        self.active()
            .segments
            .get(self.cursor)
            .map(|segment| segment.path.clone())
    }

    fn recover(&mut self, at: Duration) -> bool {
        if !self.step_down() {
            return false;
        }
        self.seek(at);
        true
    }

    fn seek(&mut self, at: Duration) {
        let segments = &self.active().segments;
        self.cursor = segments
            .iter()
            .position(|segment| at < segment.start + segment.duration)
            .unwrap_or(segments.len().saturating_sub(1));
    }

    fn observe(&mut self, fetch: Duration, segment: Duration, buffer_full: bool, max_edge: u32) {
        if self.pinned() {
            self.apply_pin();
            return;
        }
        let slower = fetch > segment / 2;
        let faster = fetch < segment / 8 && buffer_full;
        if !slower && !faster {
            return;
        }
        let eligible: Vec<usize> = self
            .variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| max_edge == 0 || variant.edge == 0 || variant.edge <= max_edge)
            .map(|(index, _)| index)
            .collect();
        let Some(place) = eligible.iter().position(|index| *index == self.current) else {
            return;
        };
        if slower {
            if let Some(next) = eligible.get(place + 1) {
                self.current = *next;
            }
        } else if place > 0 {
            self.current = eligible[place - 1];
        }
    }
}

pub fn open(path: impl AsRef<Path>) -> Result<cappy_core::VideoSource, String> {
    open_at(path.as_ref(), true, false, false).map(|(source, _)| source)
}

pub(crate) fn open_with(
    path: impl AsRef<Path>,
    audio: bool,
) -> Result<cappy_core::VideoSource, String> {
    open_at(path.as_ref(), true, audio, false).map(|(source, _)| source)
}

#[cfg(test)]
pub(crate) fn open_paused(path: impl AsRef<Path>) -> Result<cappy_core::VideoSource, String> {
    open_at(path.as_ref(), false, false, false).map(|(source, _)| source)
}

/// Playing playlist whose samples the caller pulls. No output device is started.
#[cfg(test)]
pub(crate) fn open_manual(path: &Path) -> Result<(cappy_core::VideoSource, Arc<Session>), String> {
    open_at(path, true, true, true)
}

fn open_at(
    path: &Path,
    playing: bool,
    audio: bool,
    manual: bool,
) -> Result<(cappy_core::VideoSource, Arc<Session>), String> {
    let text = read_text(path)?;
    let (mut playlist, renditions) = if text.contains("#EXT-X-STREAM-INF") {
        let master = parse_master(&text, path)?;
        let mut variants = Vec::new();
        for variant in &master.variants {
            let loaded = HlsPlaylist::media(&variant.path)?;
            let media = loaded
                .variants
                .into_iter()
                .next()
                .ok_or_else(|| "empty variant".to_owned())?;
            variants.push(Variant {
                edge: variant.edge,
                label: variant.label.clone(),
                bandwidth: variant.bandwidth,
                segments: media.segments,
                end_list: media.end_list,
            });
        }
        (
            HlsPlaylist::with_variants(path, variants, master.delay)?,
            master.audio,
        )
    } else {
        (HlsPlaylist::media(path)?, Vec::new())
    };
    if playlist
        .variants
        .first()
        .and_then(|variant| variant.segments.first())
        .is_none()
    {
        return Err("playlist has no segments".to_owned());
    }
    let tracks = audio_tracks(&renditions);
    let selected = tracks.first().map(|track| track.id);
    let session = Session::new(tracks, selected, playlist.edge, playing);
    if audio {
        session.enable_audio();
    }
    if manual {
        session.arm_manual_output(2, 48_000);
    }
    session.set_qualities(
        playlist
            .variants
            .iter()
            .map(|variant| VideoQuality {
                edge: variant.edge,
                label: variant.label.clone(),
            })
            .collect(),
    );
    let subtitles = text_renditions(&text, path);
    session.set_text(
        subtitles
            .iter()
            .enumerate()
            .map(|(index, rendition)| cappy_core::TextTrack {
                id: (index as u32) + 1,
                language: rendition.language.clone(),
                label: rendition.label.clone(),
            })
            .collect(),
        subtitles
            .iter()
            .enumerate()
            .map(|(index, rendition)| ((index as u32) + 1, rendition.cues.clone()))
            .collect(),
    );
    playlist.attach(Arc::clone(&session));
    session.set_renditions(
        renditions
            .iter()
            .enumerate()
            .map(|(index, rendition)| ((index as u32) + 1, rendition.segments.clone()))
            .collect(),
    );
    let handle = Arc::clone(&session);
    let source = launch(
        Box::new(Pipeline::new(playlist, std::sync::Arc::clone(&session))),
        session,
    )?;
    Ok((source, handle))
}

struct TextRendition {
    language: String,
    label: String,
    cues: Vec<cappy_core::TextCue>,
}

fn text_renditions(text: &str, playlist: &Path) -> Vec<TextRendition> {
    let mut renditions = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("#EXT-X-MEDIA:") else {
            continue;
        };
        if attr(rest, "TYPE").as_deref() != Some("SUBTITLES") {
            continue;
        }
        let Some(uri) = attr(rest, "URI") else {
            continue;
        };
        let language = attr(rest, "LANGUAGE").unwrap_or_else(|| "und".to_owned());
        let label = attr(rest, "NAME").unwrap_or_else(|| language.clone());
        let path = resolve(playlist, &uri);
        let cues = load_subtitle_file(&path);
        renditions.push(TextRendition {
            language,
            label,
            cues,
        });
    }
    renditions
}

fn load_subtitle_file(path: &Path) -> Vec<cappy_core::TextCue> {
    let Ok(text) = read_text(path) else {
        return Vec::new();
    };
    if text.contains("#EXTM3U") {
        let mut cues = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            cues.extend(crate::vtt::load_vtt(&resolve(path, line)));
        }
        return cues;
    }
    crate::vtt::parse_vtt(&text)
}

fn audio_tracks(renditions: &[AudioRendition]) -> Vec<AudioTrack> {
    renditions
        .iter()
        .enumerate()
        .map(|(index, rendition)| AudioTrack {
            id: (index as u32) + 1,
            language: rendition.language.clone(),
            codec: "aac".to_owned(),
            channels: 0,
            sample_rate: 0,
        })
        .collect()
}

struct VariantRef {
    path: PathBuf,
    bandwidth: u64,
    edge: u32,
    label: String,
}

struct Master {
    variants: Vec<VariantRef>,
    audio: Vec<AudioRendition>,
    delay: Duration,
}

fn parse_master(text: &str, playlist: &Path) -> Result<Master, String> {
    let mut variants = Vec::new();
    let mut audio = Vec::new();
    let mut pending: Option<(u64, u32, String)> = None;
    let mut delay = Duration::ZERO;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#CAPPY-DELAY:") {
            let seconds: f64 = rest.trim().parse().unwrap_or(0.0);
            delay = Duration::from_secs_f64(seconds);
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA:") {
            if let Some(rendition) = audio_rendition(rest, playlist) {
                audio.push(rendition);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let bandwidth = attr(rest, "BANDWIDTH")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let (edge, short) = attr(rest, "RESOLUTION")
                .map(|value| crate::session::resolution(&value))
                .unwrap_or((0, 0));
            pending = Some((bandwidth, edge, crate::session::quality_label(short)));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some((bandwidth, edge, label)) = pending.take() {
            variants.push(VariantRef {
                path: resolve(playlist, line),
                bandwidth,
                edge,
                label,
            });
        }
    }
    variants.sort_by(|left, right| right.bandwidth.cmp(&left.bandwidth));
    Ok(Master {
        variants,
        audio,
        delay,
    })
}

fn audio_rendition(attributes: &str, playlist: &Path) -> Option<AudioRendition> {
    if attr(attributes, "TYPE")? != "AUDIO" {
        return None;
    }
    let uri = attr(attributes, "URI")?;
    let language = attr(attributes, "LANGUAGE").unwrap_or_else(|| "und".to_owned());
    let media = resolve(playlist, &uri);
    let text = read_text(&media).ok()?;
    let (segments, _) = parse_media(&text, &media).ok()?;
    Some(AudioRendition {
        language,
        segments: segments
            .iter()
            .map(|segment| segment.path.display().to_string())
            .collect(),
    })
}

fn parse_media(text: &str, playlist: &Path) -> Result<(Vec<MediaSegment>, bool), String> {
    let mut segments = Vec::new();
    let mut end_list = false;
    let mut pending: Option<Duration> = None;
    let mut cursor = Duration::ZERO;
    let mut active_key: Option<crate::segment::SegmentKey> = None;
    let mut sequence: u64 = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = rest.trim().parse().unwrap_or(0);
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let method = attr(rest, "METHOD").unwrap_or_else(|| "NONE".to_owned());
            if method == "NONE" {
                active_key = None;
            } else {
                active_key = Some(parse_key(rest, playlist, sequence)?);
            }
            continue;
        }
        if line.starts_with("#EXT-X-ENDLIST") {
            end_list = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-PART:") {
            let seconds: f64 = attr(rest, "DURATION")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.0);
            let uri = attr(rest, "URI").ok_or_else(|| "part is missing a URI".to_owned())?;
            let duration = Duration::from_secs_f64(seconds);
            segments.push(MediaSegment {
                path: resolve(playlist, &uri),
                start: cursor,
                duration,
                key: active_key.clone(),
            });
            cursor += duration;
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let number = rest.split(',').next().unwrap_or("0");
            let seconds: f64 = number.parse().unwrap_or(0.0);
            pending = Some(Duration::from_secs_f64(seconds));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some(duration) = pending.take() else {
            continue;
        };
        segments.push(MediaSegment {
            path: resolve(playlist, line),
            start: cursor,
            duration,
            key: active_key.clone(),
        });
        cursor += duration;
        sequence = sequence.saturating_add(1);
    }
    Ok((segments, end_list))
}

fn parse_key(
    attributes: &str,
    playlist: &Path,
    sequence: u64,
) -> Result<crate::segment::SegmentKey, String> {
    let method = attr(attributes, "METHOD").unwrap_or_default();
    if method != "AES-128" {
        return Err(format!("unsupported segment encryption {method}"));
    }
    let uri = attr(attributes, "URI").unwrap_or_default();
    let iv = match attr(attributes, "IV") {
        Some(value) => parse_iv(&value)?,
        None => sequence_iv(sequence),
    };
    Ok(crate::segment::SegmentKey {
        method,
        uri: if uri.is_empty() {
            PathBuf::new()
        } else {
            resolve(playlist, &uri)
        },
        iv,
    })
}

fn parse_iv(value: &str) -> Result<[u8; 16], String> {
    let hex = value.trim_start_matches("0x");
    if hex.len() != 32 {
        return Err("AES-128 IV must be 16 bytes".to_owned());
    }
    let mut iv = [0; 16];
    for (index, byte) in iv.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| "AES-128 IV is not hex".to_owned())?;
    }
    Ok(iv)
}

fn sequence_iv(sequence: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

fn attr(attributes: &str, name: &str) -> Option<String> {
    for part in split_attrs(attributes) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        if key == name {
            return Some(value.trim_matches('"').to_owned());
        }
    }
    None
}

fn split_attrs(attributes: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in attributes.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            ',' if !quoted => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HlsPlaylist;
    use crate::segment::{NextSegment, SegmentSource};

    #[test]
    fn the_following_segment_is_ready_to_fetch_while_the_current_one_plays() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/hls/vod.m3u8");
        let mut playlist = HlsPlaylist::media(&path).expect("vod");
        assert!(matches!(playlist.next_segment(), NextSegment::Job(_)));
        let next = playlist.upcoming().expect("second segment");
        assert!(next.ends_with("seg1.ts"), "{}", next.display());
    }
}
