//! A video leaf's media clock. The node asks for the picture at a presentation time.
//! Decoders and playlists plug in behind [`VideoSource`]. This module does not decode.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One picture size a playlist can pin. `edge` is the long side. `label` is the short side, such as `1080p`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoQuality {
    pub edge: u32,
    pub label: String,
}

/// One audio stream discovered in the file or playlist. Selecting it does not decode audio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioTrack {
    pub id: u32,
    pub language: String,
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
}

/// A subtitle track. Cues stay out of the picture; the app draws the active one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextTrack {
    pub id: u32,
    pub language: String,
    pub label: String,
}

/// One WebVTT cue. `end` is exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

/// 4:2:0 planes. `y` is `width * height` bytes. `u` and `v` are the chroma plane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YuvPlanes {
    pub width: u32,
    pub height: u32,
    pub y: Arc<[u8]>,
    pub u: Arc<[u8]>,
    pub v: Arc<[u8]>,
}

impl YuvPlanes {
    /// A solid picture. Chroma is half the luma size, rounded up.
    pub fn solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Self {
        assert!(width > 0 && height > 0, "a video picture needs a size");
        let chroma_w = width.div_ceil(2) as usize;
        let chroma_h = height.div_ceil(2) as usize;
        Self {
            width,
            height,
            y: vec![y; width as usize * height as usize].into(),
            u: vec![u; chroma_w * chroma_h].into(),
            v: vec![v; chroma_w * chroma_h].into(),
        }
    }
}

/// The picture a node presents at one media time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoPicture {
    pub generation: u64,
    pub pts: Duration,
    pub planes: YuvPlanes,
}

struct ScriptFrame {
    pts: Duration,
    generation: u64,
    planes: YuvPlanes,
}

struct SourceState {
    id: u64,
    playing: bool,
    frames: Vec<ScriptFrame>,
    duration: Duration,
    tracks: Vec<AudioTrack>,
    selected: Option<u32>,
}

/// What a file decoder, a playlist, or a scripted clip must answer. The node owns the clock.
pub trait VideoMedia: Send + Sync {
    fn id(&self) -> u64;
    fn play(&self);
    fn pause(&self);
    fn playing(&self) -> bool;
    fn duration(&self) -> Duration;
    fn audio_tracks(&self) -> Vec<AudioTrack>;
    fn selected_audio(&self) -> Option<u32>;
    fn select_audio(&self, id: u32);
    fn picture_at(&self, at: Duration) -> VideoPicture;
    fn next_pts(&self, at: Duration) -> Option<Duration>;

    /// Jump the media clock. The default scripted source ignores this.
    fn seek(&self, _at: Duration) {}

    /// Shows `at` from pictures already decoded. Does not restart the decoder.
    fn present_at(&self, at: Duration) {
        self.seek(at);
    }

    /// Longest edge of the decoded picture. `0` means the source size.
    fn set_max_edge(&self, _edge: u32) {}

    /// Maximum presented frames per second. `0` means the source rate.
    fn set_max_fps(&self, _fps: f32) {}

    /// A small cached picture for a progress tooltip. `None` when no preview exists yet.
    fn preview_at(&self, _at: Duration) -> Option<YuvPlanes> {
        None
    }

    /// Media time last asked for by the node.
    fn presented(&self) -> Duration {
        Duration::ZERO
    }

    /// Segment list of an audio rendition. Empty when the track has no playlist.
    fn audio_segments(&self, _id: u32) -> Vec<String> {
        Vec::new()
    }

    fn text_tracks(&self) -> Vec<TextTrack> {
        Vec::new()
    }

    fn selected_text(&self) -> Option<u32> {
        None
    }

    /// Stores `id` when that track exists. An unselected track yields no cue.
    fn select_text(&self, _id: u32) {}

    /// The cue active at `at` for the selected track.
    fn cue_at(&self, _at: Duration) -> Option<String> {
        None
    }

    /// Playback speed. `1.0` is the media's own pace.
    fn playback_rate(&self) -> f32 {
        1.0
    }

    /// Stores the speed used by the node clock. Values outside `0.25..=4` are clamped.
    fn set_playback_rate(&self, _rate: f32) {}

    /// Picture sizes the playlist can pin. Empty for a single file.
    fn qualities(&self) -> Vec<VideoQuality> {
        Vec::new()
    }

    /// The pinned long edge, or `None` when the playlist is free to switch.
    fn selected_quality(&self) -> Option<u32> {
        None
    }

    /// Pins `edge` from [`qualities`](Self::qualities). `None` clears the pin.
    /// An unknown edge is ignored. The next segment uses the pin.
    fn select_quality(&self, _edge: Option<u32>) {}

    /// Media time driven by the audio device. `None` when this clip has no sound clock.
    fn audio_time(&self) -> Option<Duration> {
        None
    }

    /// Newest decoded picture. The wall clock must not run past it.
    fn latest_pts(&self) -> Option<Duration> {
        None
    }

    /// The device has played every sample it was given and none are waiting.
    fn audio_starved(&self) -> bool {
        false
    }

    /// `true` when a device was opened for this clip.
    fn audio_enabled(&self) -> bool {
        false
    }

    /// `true` when the opener asked for sound, even if no device was found.
    fn wants_audio(&self) -> bool {
        false
    }

    fn muted(&self) -> bool {
        false
    }

    /// Silences the device. The picture clock keeps moving.
    fn set_muted(&self, _muted: bool) {}

    /// Linear gain, `0` silent through `1` full. Values outside that range are clamped by the source.
    fn volume(&self) -> f32 {
        1.0
    }

    fn set_volume(&self, _volume: f32) {}

    /// Decoded picture size, once a frame has been published.
    fn pixel_size(&self) -> Option<(u32, u32)> {
        None
    }

    /// Media time the node should adopt after a seek. `None` when the node clock stands.
    fn clock_anchor(&self) -> Option<(u64, Duration)> {
        None
    }

    /// How many times the decode pool grew its buffers. Scripted clips stay at zero.
    fn pool_grows(&self) -> u32 {
        0
    }

    /// Pictures the decoder has published.
    fn published(&self) -> u64 {
        0
    }
}

/// Shared media a [`video`](crate::video) element presents.
#[derive(Clone)]
pub struct VideoSource {
    media: Arc<dyn VideoMedia>,
}

impl VideoSource {
    /// Two or more pictures already in memory. `frames` is `(pts, planes)` in order.
    ///
    /// The source starts paused. [`play`](Self::play) starts the node's clock on the next update.
    pub fn scripted(
        frames: Vec<(Duration, YuvPlanes)>,
        duration: Duration,
        tracks: Vec<AudioTrack>,
    ) -> Self {
        assert!(!frames.is_empty(), "a video needs at least one picture");
        let selected = tracks.first().map(|track| track.id);
        let frames = frames
            .into_iter()
            .enumerate()
            .map(|(index, (pts, planes))| ScriptFrame {
                pts,
                generation: (index as u64) + 1,
                planes,
            })
            .collect();
        Self::from_media(Arc::new(Scripted {
            state: Mutex::new(SourceState {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                playing: false,
                frames,
                duration,
                tracks,
                selected,
            }),
        }))
    }

    /// A decoder or playlist. The node treats it the same as a scripted clip.
    pub fn from_media(media: Arc<dyn VideoMedia>) -> Self {
        Self { media }
    }

    pub fn id(&self) -> u64 {
        self.media.id()
    }

    pub fn play(&self) {
        self.media.play();
        wake_host();
    }

    pub fn pause(&self) {
        self.media.pause();
        wake_host();
    }

    pub fn playing(&self) -> bool {
        self.media.playing()
    }

    pub fn duration(&self) -> Duration {
        self.media.duration()
    }

    pub fn audio_tracks(&self) -> Vec<AudioTrack> {
        self.media.audio_tracks()
    }

    pub fn selected_audio(&self) -> Option<u32> {
        self.media.selected_audio()
    }

    /// Stores `id` when that track exists. Playback of audio is not started.
    pub fn select_audio(&self, id: u32) {
        self.media.select_audio(id);
    }

    pub fn seek(&self, at: Duration) {
        self.media.seek(at);
        wake_host();
    }

    /// Shows `at` from pictures already decoded. [`seek`](Self::seek) is what restarts playback there.
    pub fn present_at(&self, at: Duration) {
        self.media.present_at(at);
        wake_host();
    }

    pub fn set_max_edge(&self, edge: u32) {
        self.media.set_max_edge(edge);
    }

    pub fn set_max_fps(&self, fps: f32) {
        self.media.set_max_fps(fps);
    }

    pub fn playback_rate(&self) -> f32 {
        self.media.playback_rate()
    }

    /// Stores the speed used by the node clock. Values outside `0.25..=4` are clamped.
    pub fn set_playback_rate(&self, rate: f32) {
        self.media.set_playback_rate(rate);
        wake_host();
    }

    pub fn qualities(&self) -> Vec<VideoQuality> {
        self.media.qualities()
    }

    /// The pinned long edge, or `None` when the playlist is free to switch.
    pub fn selected_quality(&self) -> Option<u32> {
        self.media.selected_quality()
    }

    /// Pins `edge` from [`qualities`](Self::qualities). `None` clears the pin.
    /// An unknown edge is ignored. The next segment uses the pin; seek to apply it now.
    pub fn select_quality(&self, edge: Option<u32>) {
        self.media.select_quality(edge);
        wake_host();
    }

    /// Media time driven by the audio device. `None` while the picture clock is on its own.
    pub fn audio_time(&self) -> Option<Duration> {
        self.media.audio_time()
    }

    pub(crate) fn latest_pts(&self) -> Option<Duration> {
        self.media.latest_pts()
    }

    pub(crate) fn audio_starved(&self) -> bool {
        self.media.audio_starved()
    }

    /// `true` when a device was opened for this clip.
    pub fn audio_enabled(&self) -> bool {
        self.media.audio_enabled()
    }

    /// `true` when the opener asked for sound, even if no device was found.
    pub fn wants_audio(&self) -> bool {
        self.media.wants_audio()
    }

    pub fn muted(&self) -> bool {
        self.media.muted()
    }

    /// Silences the device. The picture clock keeps moving.
    pub fn set_muted(&self, muted: bool) {
        self.media.set_muted(muted);
        wake_host();
    }

    /// Linear gain from `0` to `1`.
    pub fn volume(&self) -> f32 {
        self.media.volume()
    }

    /// Sets the linear gain. Values outside `0..=1` are clamped.
    pub fn set_volume(&self, volume: f32) {
        self.media.set_volume(volume);
        wake_host();
    }

    pub fn preview_at(&self, at: Duration) -> Option<YuvPlanes> {
        self.media.preview_at(at)
    }

    pub fn presented(&self) -> Duration {
        self.media.presented()
    }

    pub fn audio_segments(&self, id: u32) -> Vec<String> {
        self.media.audio_segments(id)
    }

    pub fn text_tracks(&self) -> Vec<TextTrack> {
        self.media.text_tracks()
    }

    pub fn selected_text(&self) -> Option<u32> {
        self.media.selected_text()
    }

    /// Stores `id` when that track exists.
    pub fn select_text(&self, id: u32) {
        self.media.select_text(id);
    }

    /// The cue active at `at` for the selected track. `None` when no track is selected.
    pub fn cue_at(&self, at: Duration) -> Option<String> {
        self.media.cue_at(at)
    }

    pub fn picture_at(&self, at: Duration) -> VideoPicture {
        self.media.picture_at(at)
    }

    /// Decoded picture size, once a frame has been published.
    pub fn pixel_size(&self) -> Option<(u32, u32)> {
        self.media.pixel_size()
    }

    pub(crate) fn next_pts(&self, at: Duration) -> Option<Duration> {
        self.media.next_pts(at)
    }

    pub(crate) fn clock_anchor(&self) -> Option<(u64, Duration)> {
        self.media.clock_anchor()
    }

    pub fn pool_grows(&self) -> u32 {
        self.media.pool_grows()
    }

    pub fn published(&self) -> u64 {
        self.media.published()
    }
}

struct Scripted {
    state: Mutex<SourceState>,
}

impl VideoMedia for Scripted {
    fn id(&self) -> u64 {
        self.state.lock().expect("video source lock").id
    }

    fn play(&self) {
        self.state.lock().expect("video source lock").playing = true;
    }

    fn pause(&self) {
        self.state.lock().expect("video source lock").playing = false;
    }

    fn playing(&self) -> bool {
        self.state.lock().expect("video source lock").playing
    }

    fn duration(&self) -> Duration {
        self.state.lock().expect("video source lock").duration
    }

    fn audio_tracks(&self) -> Vec<AudioTrack> {
        self.state.lock().expect("video source lock").tracks.clone()
    }

    fn selected_audio(&self) -> Option<u32> {
        self.state.lock().expect("video source lock").selected
    }

    fn select_audio(&self, id: u32) {
        let mut state = self.state.lock().expect("video source lock");
        if state.tracks.iter().any(|track| track.id == id) {
            state.selected = Some(id);
        }
    }

    fn picture_at(&self, at: Duration) -> VideoPicture {
        let state = self.state.lock().expect("video source lock");
        let frame = state
            .frames
            .iter()
            .rev()
            .find(|frame| frame.pts <= at)
            .unwrap_or(&state.frames[0]);
        VideoPicture {
            generation: frame.generation,
            pts: frame.pts,
            planes: frame.planes.clone(),
        }
    }

    fn next_pts(&self, at: Duration) -> Option<Duration> {
        let state = self.state.lock().expect("video source lock");
        state
            .frames
            .iter()
            .find(|frame| frame.pts > at)
            .map(|frame| frame.pts)
    }
}

/// Media time that passes during one wall-clock step at `rate`. A rate of `1` returns `wall`.
pub(crate) fn media_elapsed(wall: Duration, rate: f32) -> Duration {
    if (rate - 1.0).abs() <= f32::EPSILON {
        return wall;
    }
    let rate = rate.clamp(0.25, 4.0);
    Duration::from_secs_f64(wall.as_secs_f64() * f64::from(rate))
}

/// Wall time to wait while `media` elapses at `rate`. A rate of `1` returns `media`.
pub(crate) fn wall_wait(media: Duration, rate: f32) -> Duration {
    if (rate - 1.0).abs() <= f32::EPSILON {
        return media;
    }
    let rate = rate.clamp(0.25, 4.0);
    Duration::from_secs_f64(media.as_secs_f64() / f64::from(rate))
}

fn wake_host() {
    let runtime = crate::reactive::runtime_if_any();
    if let Some(runtime) = runtime {
        runtime.wake_host();
    }
}

#[cfg(test)]
mod tests {
    use super::{media_elapsed, wall_wait};
    use std::time::Duration;

    #[test]
    fn a_unit_rate_keeps_the_wall_clock_and_a_faster_rate_shortens_the_wait() {
        let step = Duration::from_millis(100);
        assert_eq!(media_elapsed(step, 1.0), step);
        assert_eq!(wall_wait(step, 1.0), step);
        let doubled = media_elapsed(step, 2.0);
        assert!((doubled.as_secs_f64() - 0.2).abs() < 0.002);
        let wait = wall_wait(Duration::from_millis(200), 2.0);
        assert!((wait.as_secs_f64() - 0.1).abs() < 0.002);
    }
}
