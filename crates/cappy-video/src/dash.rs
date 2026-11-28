//! A DASH adapter. It turns an MPD into the same segment jobs HLS uses.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cappy_core::{AudioTrack, VideoQuality};

use crate::file::launch;
use crate::segment::{NextSegment, Pipeline, SegmentJob, SegmentSource};
use crate::session::Session;

struct MediaSegment {
    path: PathBuf,
    start: Duration,
    duration: Duration,
}

struct Variant {
    edge: u32,
    label: String,
    bandwidth: u64,
    init: Option<PathBuf>,
    segments: Vec<MediaSegment>,
}

struct AudioRendition {
    language: String,
    segments: Vec<String>,
}

pub(crate) struct DashPlaylist {
    variants: Vec<Variant>,
    current: usize,
    cursor: usize,
    started: bool,
    session: Option<Arc<Session>>,
}

impl DashPlaylist {
    fn attach(&mut self, session: Arc<Session>) {
        self.session = Some(session);
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
        self.current = self.best(max_edge).unwrap_or(0);
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

impl SegmentSource for DashPlaylist {
    fn next_segment(&mut self) -> NextSegment {
        if !self.started {
            self.choose_initial();
            self.started = true;
        } else {
            self.apply_pin();
        }
        let Some(segment) = self.active().segments.get(self.cursor) else {
            return NextSegment::Done;
        };
        let job = SegmentJob {
            path: segment.path.clone(),
            start: segment.start,
            duration: segment.duration,
            key: None,
            delay: Duration::ZERO,
            init: self.active().init.clone(),
        };
        self.cursor += 1;
        NextSegment::Job(job)
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
    open_at(path.as_ref(), true, false)
}

pub(crate) fn open_with(
    path: impl AsRef<Path>,
    audio: bool,
) -> Result<cappy_core::VideoSource, String> {
    open_at(path.as_ref(), true, audio)
}

#[cfg(test)]
pub(crate) fn open_paused(path: impl AsRef<Path>) -> Result<cappy_core::VideoSource, String> {
    open_at(path.as_ref(), false, false)
}

fn open_at(path: &Path, playing: bool, audio: bool) -> Result<cappy_core::VideoSource, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let parsed = parse_mpd(&text, path)?;
    let mut playlist = parsed.playlist;
    if playlist
        .variants
        .first()
        .and_then(|variant| variant.segments.first())
        .is_none()
    {
        return Err("mpd has no segments".to_owned());
    }
    let tracks = audio_tracks(&parsed.audio);
    let selected = tracks.first().map(|track| track.id);
    let duration = playlist
        .variants
        .first()
        .and_then(|variant| variant.segments.last())
        .map(|segment| segment.start + segment.duration)
        .unwrap_or_default();
    let session = Session::new(tracks, selected, duration, playing);
    if audio {
        session.enable_audio();
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
    session.set_text(
        parsed
            .text
            .iter()
            .enumerate()
            .map(|(index, (language, _))| cappy_core::TextTrack {
                id: (index as u32) + 1,
                language: language.clone(),
                label: language.clone(),
            })
            .collect(),
        parsed
            .text
            .iter()
            .enumerate()
            .map(|(index, (_, cues))| ((index as u32) + 1, cues.clone()))
            .collect(),
    );
    playlist.attach(Arc::clone(&session));
    session.set_renditions(
        parsed
            .audio
            .iter()
            .enumerate()
            .map(|(index, rendition)| ((index as u32) + 1, rendition.segments.clone()))
            .collect(),
    );
    launch(
        Box::new(Pipeline::new(playlist, Arc::clone(&session))),
        session,
    )
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

struct Parsed {
    playlist: DashPlaylist,
    audio: Vec<AudioRendition>,
    text: Vec<(String, Vec<cappy_core::TextCue>)>,
}

struct SetBuilder {
    kind: SetKind,
    language: String,
    reps: Vec<RepBuilder>,
}

struct RepBuilder {
    bandwidth: u64,
    edge: u32,
    label: String,
    init: Option<PathBuf>,
    list_duration: Duration,
    segments: Vec<MediaSegment>,
    cursor: Duration,
}

enum SetKind {
    Video,
    Audio,
    Text,
}

fn parse_mpd(text: &str, mpd: &Path) -> Result<Parsed, String> {
    let mut sets: Vec<SetBuilder> = Vec::new();
    let mut current: Option<SetBuilder> = None;
    let mut rep: Option<RepBuilder> = None;
    for tag in tags(text) {
        match tag {
            Tag::Open { name, attrs } if name == "adaptationset" => {
                current = Some(SetBuilder {
                    kind: set_kind(&attrs),
                    language: attr(&attrs, "lang")
                        .or_else(|| attr(&attrs, "language"))
                        .unwrap_or_else(|| "und".to_owned()),
                    reps: Vec::new(),
                });
            }
            Tag::Open { name, attrs } if name == "representation" => {
                let width = attr(&attrs, "width")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let height = attr(&attrs, "height")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let short = if width == 0 || height == 0 {
                    width.max(height)
                } else {
                    width.min(height)
                };
                rep = Some(RepBuilder {
                    bandwidth: attr(&attrs, "bandwidth")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                    edge: width.max(height),
                    label: crate::session::quality_label(short),
                    init: None,
                    list_duration: Duration::ZERO,
                    segments: Vec::new(),
                    cursor: Duration::ZERO,
                });
            }
            Tag::Open { name, attrs } if name == "segmentlist" => {
                if let Some(rep) = rep.as_mut() {
                    let seconds: f64 = attr(&attrs, "duration")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0.0);
                    rep.list_duration = Duration::from_secs_f64(seconds);
                }
            }
            Tag::Open { name, attrs } if name == "initialization" => {
                if let Some(rep) = rep.as_mut() {
                    if let Some(source) = attr(&attrs, "sourceurl") {
                        rep.init = Some(resolve(mpd, &source));
                    }
                }
            }
            Tag::Open { name, attrs } if name == "segmenturl" => {
                let Some(rep) = rep.as_mut() else {
                    continue;
                };
                let Some(media) = attr(&attrs, "media") else {
                    continue;
                };
                let seconds: f64 = attr(&attrs, "duration")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or_else(|| rep.list_duration.as_secs_f64());
                let duration = Duration::from_secs_f64(seconds);
                rep.segments.push(MediaSegment {
                    path: resolve(mpd, &media),
                    start: rep.cursor,
                    duration,
                });
                rep.cursor += duration;
            }
            Tag::Close(name) if name == "representation" => {
                if let (Some(set), Some(rep)) = (current.as_mut(), rep.take()) {
                    set.reps.push(rep);
                }
            }
            Tag::Close(name) if name == "adaptationset" => {
                if let Some(set) = current.take() {
                    sets.push(set);
                }
            }
            _ => {}
        }
    }
    let mut variants = Vec::new();
    let mut audio = Vec::new();
    let mut text = Vec::new();
    for set in sets {
        match set.kind {
            SetKind::Text => {
                if let Some(rep) = set.reps.into_iter().next() {
                    let cues = rep
                        .segments
                        .iter()
                        .flat_map(|segment| crate::vtt::load_vtt(&segment.path))
                        .collect();
                    text.push((set.language, cues));
                }
            }
            SetKind::Video => {
                for rep in set.reps {
                    variants.push(Variant {
                        edge: rep.edge,
                        label: rep.label,
                        bandwidth: rep.bandwidth,
                        init: rep.init,
                        segments: rep.segments,
                    });
                }
            }
            SetKind::Audio => {
                if let Some(rep) = set.reps.into_iter().next() {
                    audio.push(AudioRendition {
                        language: set.language,
                        segments: rep
                            .segments
                            .iter()
                            .map(|segment| segment.path.display().to_string())
                            .collect(),
                    });
                }
            }
        }
    }
    variants.sort_by(|left, right| right.bandwidth.cmp(&left.bandwidth));
    if variants.is_empty() {
        return Err("mpd has no video representation".to_owned());
    }
    Ok(Parsed {
        playlist: DashPlaylist {
            variants,
            current: 0,
            cursor: 0,
            started: false,
            session: None,
        },
        audio,
        text,
    })
}

fn set_kind(attrs: &str) -> SetKind {
    let content = attr(attrs, "contenttype").unwrap_or_default();
    let mime = attr(attrs, "mimetype").unwrap_or_default();
    let label = format!("{content} {mime}").to_ascii_lowercase();
    if label.contains("audio") {
        SetKind::Audio
    } else if label.contains("text") || label.contains("subtitle") || label.contains("vtt") {
        SetKind::Text
    } else if label.contains("video") || label.contains("mp2t") || label.contains("mp4") {
        SetKind::Video
    } else {
        SetKind::Video
    }
}

enum Tag {
    Open { name: String, attrs: String },
    Close(String),
}

fn tags(text: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let Some(end) = text[index..].find('>') else {
            break;
        };
        let raw = &text[index + 1..index + end];
        index += end + 1;
        let raw = raw.trim();
        if raw.starts_with('!') || raw.starts_with('?') {
            continue;
        }
        if let Some(name) = raw.strip_prefix('/') {
            tags.push(Tag::Close(name.trim().to_ascii_lowercase()));
            continue;
        }
        let raw = raw.trim_end_matches('/');
        let (name, attrs) = raw.split_once(char::is_whitespace).unwrap_or((raw, ""));
        tags.push(Tag::Open {
            name: name.to_ascii_lowercase(),
            attrs: attrs.to_owned(),
        });
    }
    tags
}

fn attr(attributes: &str, name: &str) -> Option<String> {
    let mut rest = attributes;
    while let Some(start) = rest.find('=') {
        let key = rest[..start]
            .split_whitespace()
            .last()?
            .trim()
            .to_ascii_lowercase();
        rest = &rest[start + 1..];
        let rest_trim = rest.trim_start();
        let value = if let Some(stripped) = rest_trim.strip_prefix('"') {
            let end = stripped.find('"')?;
            rest = &stripped[end + 1..];
            stripped[..end].to_owned()
        } else if let Some(stripped) = rest_trim.strip_prefix('\'') {
            let end = stripped.find('\'')?;
            rest = &stripped[end + 1..];
            stripped[..end].to_owned()
        } else {
            let end = rest_trim
                .find(char::is_whitespace)
                .unwrap_or(rest_trim.len());
            rest = &rest_trim[end..];
            rest_trim[..end].to_owned()
        };
        if key == name {
            return Some(value);
        }
    }
    None
}

fn resolve(playlist: &Path, reference: &str) -> PathBuf {
    let reference = Path::new(reference);
    if reference.is_absolute() {
        return reference.to_path_buf();
    }
    playlist
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(reference)
}
