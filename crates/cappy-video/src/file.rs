//! An MP4 adapter. It reads the file and yields encoded packets. It does not decode them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::thread::JoinHandle;
use std::time::Duration;

use cappy_core::{AudioTrack, VideoMedia, VideoPicture, VideoSource};
use ffmpeg_next as ffmpeg;

use crate::decode::FfmpegDecoder;
use crate::packet::{
    AudioCodec, AudioFacts, EncodedPacket, PacketSource, Pull, StreamConfig, VideoCodec,
};
use crate::session::{self, Session};
use crate::worker;

static FFMPEG: Once = Once::new();

pub struct FileMedia {
    session: Arc<Session>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for FileMedia {
    fn drop(&mut self) {
        if let Ok(mut control) = self.session.control.lock() {
            control.stop = true;
        }
        self.session.notify();
        // The worker may be inside a segment download. Joining here would freeze the UI until
        // that read returns. A side thread reaps it after the current fetch notices `stop`.
        if let Some(thread) = self.thread.lock().ok().and_then(|mut slot| slot.take()) {
            std::thread::spawn(move || {
                let _ = thread.join();
            });
        }
    }
}

/// What [`open_with`](open_with) turns on. Both flags default off, so a picture-only open
/// does not scan keyframes or open a device.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenOptions {
    pub audio: bool,
    pub previews: bool,
}

/// Opens a locator and starts the decode worker playing.
/// A path ending in `.m3u8`, or a file that starts as a playlist, uses HLS.
/// A path ending in `.mpd`, or a file whose content is an MPD, uses DASH.
/// Sound and the keyframe scan stay off. Use [`open_with`](open_with) to turn them on.
pub fn open(path: impl AsRef<Path>) -> Result<VideoSource, String> {
    open_with(path, OpenOptions::default())
}

/// Same locator rules as [`open`](open). `audio` opens a device when the file has AAC or MP3.
/// `previews` reads keyframes once so [`VideoSource::preview_at`](cappy_core::VideoSource::preview_at) can answer.
pub fn open_with(path: impl AsRef<Path>, options: OpenOptions) -> Result<VideoSource, String> {
    match locator_kind(path.as_ref()) {
        LocatorKind::Hls => crate::hls::open_with(path, options.audio),
        LocatorKind::Dash => crate::dash::open_with(path, options.audio),
        LocatorKind::File => open_player(path.as_ref(), true, options),
    }
}

enum LocatorKind {
    File,
    Hls,
    Dash,
}

fn locator_kind(path: &Path) -> LocatorKind {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "m3u8" => return LocatorKind::Hls,
        Some(ext) if ext == "mpd" => return LocatorKind::Dash,
        _ => {}
    }
    let mut header = [0_u8; 64];
    let Ok(mut file) = std::fs::File::open(path) else {
        return LocatorKind::File;
    };
    use std::io::Read;
    let Ok(read) = file.read(&mut header) else {
        return LocatorKind::File;
    };
    let header = String::from_utf8_lossy(&header[..read]);
    let header = header.trim_start();
    if header.starts_with("#EXTM3U") {
        LocatorKind::Hls
    } else if header.contains("<MPD") || header.contains("<mpd") {
        LocatorKind::Dash
    } else {
        LocatorKind::File
    }
}

/// Opens `path` paused, so a test can set limits before the first picture.
#[cfg(test)]
pub(crate) fn open_paused(path: impl AsRef<Path>) -> Result<VideoSource, String> {
    open_player(path.as_ref(), false, OpenOptions::default())
}

fn open_player(path: &Path, playing: bool, options: OpenOptions) -> Result<VideoSource, String> {
    open_session(path, playing, options, false).map(|(source, _)| source)
}

fn open_session(
    path: &Path,
    playing: bool,
    options: OpenOptions,
    manual_audio: bool,
) -> Result<(VideoSource, Arc<Session>), String> {
    ensure_ffmpeg()?;
    let probed = probe(path)?;
    let mut source = open_parts(path)?;
    let config = source.config.clone();
    FfmpegDecoder::open(&config)?;
    let session = Session::new(probed.tracks, probed.selected, probed.duration, playing);
    if options.audio {
        session.enable_audio();
    }
    if manual_audio {
        session.arm_manual_output(2, 48_000);
    }
    if options.previews {
        session.enable_previews();
    }
    let text = crate::vtt::from_container(path);
    session.set_text(text.tracks, text.cues);
    source.attach(Arc::clone(&session));
    let worker_session = Arc::clone(&session);
    let thread =
        std::thread::spawn(move || worker::run(Box::new(source), Some(config), worker_session));
    Ok((
        VideoSource::from_media(Arc::new(FileMedia {
            session: Arc::clone(&session),
            thread: Mutex::new(Some(thread)),
        })),
        session,
    ))
}

/// Opens a playing file whose samples the caller pulls. No output device is started.
#[cfg(test)]
pub(crate) fn open_manual(path: &Path) -> Result<(VideoSource, Arc<Session>), String> {
    open_session(
        path,
        true,
        OpenOptions {
            audio: true,
            previews: false,
        },
        true,
    )
}

/// The short MP4 shipped with this crate.
pub fn sample_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/sample.mp4")
}

/// Reads every video packet from a container ffmpeg can open.
pub(crate) struct Demuxed {
    pub config: StreamConfig,
    pub packets: Vec<EncodedPacket>,
    pub audio: Option<AudioFacts>,
}

pub(crate) fn demux_path(path: &Path) -> Result<(StreamConfig, Vec<EncodedPacket>), String> {
    let demuxed = demux_media(path, false)?;
    Ok((demuxed.config, demuxed.packets))
}

pub(crate) fn demux_media(path: &Path, keep_audio: bool) -> Result<Demuxed, String> {
    ensure_ffmpeg()?;
    let mut source = open_parts(path)?;
    source.keep_audio = keep_audio;
    let config = source.config.clone();
    let audio = keep_audio.then(|| source.audio.clone()).flatten();
    let mut packets = Vec::new();
    loop {
        match source.pull() {
            Pull::Packet(packet) => packets.push(packet),
            Pull::Reset | Pull::Idle => continue,
            Pull::Ended => break,
        }
    }
    Ok(Demuxed {
        config,
        packets,
        audio,
    })
}

/// Reads every video packet. The returned config can open a decoder the caller already has.
pub(crate) fn load_packets(path: &Path) -> Result<(StreamConfig, Vec<EncodedPacket>), String> {
    demux_path(path)
}

fn ensure_ffmpeg() -> Result<(), String> {
    let mut error = None;
    FFMPEG.call_once(|| {
        if let Err(failure) = ffmpeg::init() {
            error = Some(failure.to_string());
        }
    });
    match error {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

struct Probe {
    tracks: Vec<AudioTrack>,
    selected: Option<u32>,
    duration: Duration,
}

fn probe(path: &Path) -> Result<Probe, String> {
    let input = ffmpeg::format::input(path).map_err(|error| error.to_string())?;
    let duration = Duration::from_micros(input.duration().max(0) as u64);
    let mut tracks = Vec::new();
    for stream in input.streams() {
        if stream.parameters().medium() != ffmpeg::media::Type::Audio {
            continue;
        }
        let id = stream.index() as u32;
        let language = stream
            .metadata()
            .get("language")
            .unwrap_or("und")
            .to_owned();
        let (codec, channels, rate) = audio_facts(&stream);
        tracks.push(AudioTrack {
            id,
            language,
            codec,
            channels,
            sample_rate: rate,
        });
    }
    let selected = tracks.first().map(|track| track.id);
    Ok(Probe {
        tracks,
        selected,
        duration,
    })
}

fn audio_facts(stream: &ffmpeg::format::stream::Stream) -> (String, u16, u32) {
    let Ok(context) = ffmpeg::codec::context::Context::from_parameters(stream.parameters()) else {
        return ("unknown".to_owned(), 0, 0);
    };
    let Ok(audio) = context.decoder().audio() else {
        return ("unknown".to_owned(), 0, 0);
    };
    (audio.id().name().to_owned(), audio.channels(), audio.rate())
}

struct FileSource {
    input: ffmpeg::format::context::Input,
    index: usize,
    config: StreamConfig,
    audio_index: Option<usize>,
    audio: Option<AudioFacts>,
    keep_audio: bool,
    session: Option<Arc<Session>>,
}

fn open_parts(path: &Path) -> Result<FileSource, String> {
    let input = ffmpeg::format::input(path).map_err(|error| error.to_string())?;
    let stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| "no video stream".to_owned())?;
    let index = stream.index();
    let time_base = stream.time_base();
    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|error| error.to_string())?;
    let extradata = context.extradata().unwrap_or_default().to_vec();
    let config = StreamConfig {
        codec: VideoCodec::H264,
        time_base_num: time_base.numerator(),
        time_base_den: time_base.denominator(),
        extradata,
    };
    let (audio_index, audio) = first_audio(&input);
    Ok(FileSource {
        input,
        index,
        config,
        audio_index,
        audio,
        keep_audio: false,
        session: None,
    })
}

fn first_audio(input: &ffmpeg::format::context::Input) -> (Option<usize>, Option<AudioFacts>) {
    for stream in input.streams() {
        if stream.parameters().medium() != ffmpeg::media::Type::Audio {
            continue;
        }
        let Some(codec) = audio_codec(stream.parameters().id()) else {
            continue;
        };
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters());
        let extradata = context
            .ok()
            .and_then(|context| context.extradata().map(|bytes| bytes.to_vec()))
            .unwrap_or_default();
        let facts = stream_audio(&stream);
        return (
            Some(stream.index()),
            Some(AudioFacts {
                codec,
                rate: facts.2,
                channels: facts.1,
                extradata,
            }),
        );
    }
    (None, None)
}

fn audio_codec(id: ffmpeg::codec::Id) -> Option<AudioCodec> {
    if id == ffmpeg::codec::Id::AAC {
        Some(AudioCodec::Aac)
    } else if id == ffmpeg::codec::Id::MP3 {
        Some(AudioCodec::Mp3)
    } else {
        None
    }
}

fn stream_audio(stream: &ffmpeg::format::stream::Stream) -> (String, u16, u32) {
    audio_facts(stream)
}

impl FileSource {
    fn attach(&mut self, session: Arc<Session>) {
        self.session = Some(session);
    }
}

impl PacketSource for FileSource {
    fn can_rewind(&self) -> bool {
        true
    }

    fn rewind(&mut self) {
        let _ = self.input.seek(0, ..0);
    }

    fn pull(&mut self) -> Pull {
        if let Some(at) = self
            .session
            .as_ref()
            .and_then(|session| session.take_seek())
        {
            let micros = at.as_micros().min(i64::MAX as u128) as i64;
            let _ = self.input.seek(micros, ..micros);
            return Pull::Reset;
        }
        let keep_audio = self.keep_audio
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.wants_audio());
        let audio_index = self
            .session
            .as_ref()
            .and_then(|session| session.selected_audio())
            .map(|id| id as usize)
            .or(self.audio_index);
        for (stream, packet) in self.input.packets() {
            let index = stream.index();
            let audio = keep_audio && Some(index) == audio_index;
            if index != self.index && !audio {
                continue;
            }
            let data = packet.data().unwrap_or_default().to_vec();
            return Pull::Packet(EncodedPacket {
                data,
                pts: packet.pts(),
                key: packet.is_key(),
                audio,
            });
        }
        Pull::Ended
    }

    fn audio_facts(&self) -> Option<AudioFacts> {
        self.audio.clone()
    }
}

impl VideoMedia for FileMedia {
    fn id(&self) -> u64 {
        self.session.control.lock().expect("video").id
    }

    fn play(&self) {
        let mut control = self.session.control.lock().expect("video");
        control.playing = true;
        control.finished = false;
        drop(control);
        self.session.play_speaker();
        self.session.notify();
    }

    fn pause(&self) {
        self.session.control.lock().expect("video").playing = false;
        self.session.pause_speaker();
        self.session.notify();
    }

    fn playing(&self) -> bool {
        self.session.control.lock().expect("video").playing
    }

    fn duration(&self) -> Duration {
        self.session.control.lock().expect("video").duration
    }

    fn audio_tracks(&self) -> Vec<AudioTrack> {
        self.session.control.lock().expect("video").tracks.clone()
    }

    fn selected_audio(&self) -> Option<u32> {
        self.session.control.lock().expect("video").selected
    }

    fn select_audio(&self, id: u32) {
        let mut control = self.session.control.lock().expect("video");
        if control.tracks.iter().any(|track| track.id == id) {
            control.selected = Some(id);
        }
    }

    fn picture_at(&self, at: Duration) -> VideoPicture {
        let mut control = self.session.control.lock().expect("video");
        control.presented = at;
        let pinned = control.hold.is_some() || control.accurate.is_some();
        let queued = if pinned {
            None
        } else {
            let queued = control
                .queue
                .iter()
                .rev()
                .find(|frame| frame.pts <= at)
                .cloned();
            if let Some(shown) = &queued {
                let pts = shown.pts;
                control.queue.retain(|frame| frame.pts >= pts);
            }
            queued
        };
        let archived = session::archived(&control, at);
        let chosen = match (queued, archived) {
            (Some(queued), Some(archived)) if queued.pts >= archived.pts => Some(queued),
            (Some(_), Some(archived)) => Some(archived),
            (Some(queued), None) => Some(queued),
            (None, Some(archived)) => Some(archived),
            (None, None) => None,
        };
        control.shown_pts = chosen.as_ref().map(|picture| picture.pts);
        drop(control);
        if !pinned {
            self.session.notify();
        }
        chosen.unwrap_or_else(session::black_picture)
    }

    fn next_pts(&self, at: Duration) -> Option<Duration> {
        let control = self.session.control.lock().expect("video");
        if let Some(next) = session::soonest_picture(&control, at) {
            return Some(next);
        }
        if control.playing && !control.finished {
            return Some(at + Duration::from_millis(16));
        }
        None
    }

    fn seek(&self, at: Duration) {
        let mut control = self.session.control.lock().expect("video");
        control.hold = None;
        // A stored frame can play immediately. Waiting for a new decode pins the clock at 0:00.
        let cached = session::nearest_pts(&control, at).is_some();
        control.accurate = (!cached).then_some(at);
        control.seek = Some(at);
        control.anchor_epoch = control.anchor_epoch.wrapping_add(1);
        control.anchor_at = at;
        control.queue.clear();
        control.finished = false;
        control.presented = at;
        drop(control);
        self.session.clear_pcm(at);
        self.session.notify();
    }

    fn present_at(&self, at: Duration) {
        let mut control = self.session.control.lock().expect("video");
        control.hold = Some(at);
        control.anchor_at = at;
        control.presented = at;
        // Same stored frame: the playhead moves and the picture stays, so the scrub does not reupload.
        if let Some(next) = session::nearest_pts(&control, at)
            && control.shown_pts != Some(next)
        {
            control.anchor_epoch = control.anchor_epoch.wrapping_add(1);
        }
    }

    fn pixel_size(&self) -> Option<(u32, u32)> {
        self.session.pixel_size()
    }

    fn audio_time(&self) -> Option<Duration> {
        self.session.audio_time()
    }

    fn latest_pts(&self) -> Option<Duration> {
        self.session.latest_pts()
    }

    fn audio_starved(&self) -> bool {
        self.session.audio_starved()
    }

    fn audio_enabled(&self) -> bool {
        self.session.audio_enabled()
    }

    fn wants_audio(&self) -> bool {
        self.session.wants_audio()
    }

    fn muted(&self) -> bool {
        self.session.muted()
    }

    fn set_muted(&self, muted: bool) {
        let was = self.session.muted();
        self.session.set_muted(muted);
        if was && !muted {
            let at = self.presented();
            self.seek(at);
        }
    }

    fn volume(&self) -> f32 {
        self.session.volume()
    }

    fn set_volume(&self, volume: f32) {
        self.session.set_volume(volume);
    }

    fn set_max_edge(&self, edge: u32) {
        self.session.control.lock().expect("video").max_edge = edge;
    }

    fn set_max_fps(&self, fps: f32) {
        self.session.control.lock().expect("video").max_fps = fps;
    }

    fn playback_rate(&self) -> f32 {
        self.session.rate()
    }

    fn set_playback_rate(&self, rate: f32) {
        self.session.set_rate(rate);
    }

    fn qualities(&self) -> Vec<cappy_core::VideoQuality> {
        self.session.qualities()
    }

    fn selected_quality(&self) -> Option<u32> {
        self.session.pinned_quality()
    }

    fn select_quality(&self, edge: Option<u32>) {
        self.session.pin_quality(edge);
    }

    fn presented(&self) -> Duration {
        self.session.control.lock().expect("video").presented
    }

    fn text_tracks(&self) -> Vec<cappy_core::TextTrack> {
        self.session
            .control
            .lock()
            .expect("video")
            .text_tracks
            .clone()
    }

    fn selected_text(&self) -> Option<u32> {
        self.session.control.lock().expect("video").selected_text
    }

    fn select_text(&self, id: u32) {
        let mut control = self.session.control.lock().expect("video");
        if control.text_tracks.iter().any(|track| track.id == id) {
            control.selected_text = Some(id);
        }
    }

    fn cue_at(&self, at: Duration) -> Option<String> {
        self.session.cue_at(at)
    }

    fn audio_segments(&self, id: u32) -> Vec<String> {
        self.session
            .control
            .lock()
            .expect("video")
            .renditions
            .iter()
            .find(|(track, _)| *track == id)
            .map(|(_, segments)| segments.clone())
            .unwrap_or_default()
    }

    fn preview_at(&self, at: Duration) -> Option<cappy_core::YuvPlanes> {
        let control = self.session.control.lock().expect("video");
        control
            .previews
            .iter()
            .rev()
            .find(|(pts, _)| *pts <= at)
            .map(|(_, planes)| planes.clone())
    }

    fn clock_anchor(&self) -> Option<(u64, Duration)> {
        let control = self.session.control.lock().expect("video");
        Some((control.anchor_epoch, control.anchor_at))
    }

    fn pool_grows(&self) -> u32 {
        self.session.control.lock().expect("video").pool_grows
    }

    fn published(&self) -> u64 {
        self.session.control.lock().expect("video").published
    }
}

/// Starts the worker before a segment has been fetched. The worker opens the decoder from the
/// first demuxed segment, so the caller does not download that segment twice.
pub(crate) fn launch(
    source: Box<dyn crate::packet::PacketSource>,
    session: Arc<Session>,
) -> Result<VideoSource, String> {
    ensure_ffmpeg()?;
    let worker_session = Arc::clone(&session);
    let thread = std::thread::spawn(move || worker::run(source, None, worker_session));
    Ok(VideoSource::from_media(Arc::new(FileMedia {
        session,
        thread: Mutex::new(Some(thread)),
    })))
}

/// Packets in the caller's order, with a manual device. Used to put video ahead of its audio.
#[cfg(test)]
pub(crate) fn play_ordered(
    config: StreamConfig,
    packets: Vec<EncodedPacket>,
    audio: Option<AudioFacts>,
    duration: Duration,
) -> Result<(VideoSource, Arc<Session>), String> {
    ensure_ffmpeg()?;
    FfmpegDecoder::open(&config)?;
    let session = Session::new(Vec::new(), None, duration, true);
    session.enable_audio();
    session.arm_manual_output(2, 48_000);
    let source = crate::packet::ListSource::with_audio(packets, audio);
    let worker_session = Arc::clone(&session);
    let thread =
        std::thread::spawn(move || worker::run(Box::new(source), Some(config), worker_session));
    Ok((
        VideoSource::from_media(Arc::new(FileMedia {
            session: Arc::clone(&session),
            thread: Mutex::new(Some(thread)),
        })),
        session,
    ))
}

/// Starts a player from packets the caller already has. The caller does not name a decoder.
#[cfg(test)]
pub(crate) fn play_packets(
    config: StreamConfig,
    packets: Vec<EncodedPacket>,
    tracks: Vec<AudioTrack>,
    duration: Duration,
) -> Result<VideoSource, String> {
    ensure_ffmpeg()?;
    FfmpegDecoder::open(&config)?;
    let session = Session::new(tracks, None, duration, true);
    let source = crate::packet::ListSource::new(packets);
    let worker_session = Arc::clone(&session);
    let thread =
        std::thread::spawn(move || worker::run(Box::new(source), Some(config), worker_session));
    Ok(VideoSource::from_media(Arc::new(FileMedia {
        session,
        thread: Mutex::new(Some(thread)),
    })))
}
