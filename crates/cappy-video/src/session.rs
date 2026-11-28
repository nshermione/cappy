//! Player state shared by the handle, the adapter, and the decode worker.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::sound::SpeakerCmd;

use cappy_core::{AudioTrack, TextCue, TextTrack, VideoPicture, VideoQuality, YuvPlanes};

use crate::decode::FrameOut;

pub(crate) struct Session {
    pub control: Mutex<Control>,
    pub wake: Condvar,
    pcm: Mutex<Pcm>,
    speaker: Mutex<Option<Sender<SpeakerCmd>>>,
}

struct Pcm {
    live: bool,
    muted: bool,
    channels: u16,
    rate: u32,
    speed: f32,
    gain: f32,
    samples: VecDeque<f32>,
    played: u64,
    anchor: Duration,
    started: bool,
    credit: f32,
    held: Vec<f32>,
}

pub(crate) struct Control {
    pub playing: bool,
    pub stop: bool,
    pub seek: Option<Duration>,
    /// Scrub is showing a stored picture. The live queue stays out of the way.
    pub hold: Option<Duration>,
    /// Decoder is catching up to this time. Early frames stay off the screen.
    pub accurate: Option<Duration>,
    pub pixels: Option<(u32, u32)>,
    pub max_edge: u32,
    pub max_fps: f32,
    pub rate: f32,
    wants_audio: bool,
    wants_previews: bool,
    qualities: Vec<VideoQuality>,
    pinned_quality: Option<u32>,
    pub presented: Duration,
    pub queue: VecDeque<VideoPicture>,
    pub tracks: Vec<AudioTrack>,
    pub selected: Option<u32>,
    pub duration: Duration,
    pub id: u64,
    pub pool_grows: u32,
    pub published: u64,
    pub anchor_epoch: u64,
    pub anchor_at: Duration,
    /// The test harness consumes [`Session::render_audio`] itself. No device is opened.
    manual_output: bool,
    /// Picture currently on screen. Scrub skips a refresh when this frame already matches.
    pub shown_pts: Option<Duration>,
    pub previews: Vec<(Duration, YuvPlanes)>,
    pub finished: bool,
    pub renditions: Vec<(u32, Vec<String>)>,
    pub text_tracks: Vec<TextTrack>,
    pub selected_text: Option<u32>,
    pub cues: Vec<(u32, Vec<TextCue>)>,
    /// Pictures already decoded, so a seek into watched media can show one immediately.
    archive: Vec<(Duration, YuvPlanes)>,
    archive_bytes: usize,
}

impl Session {
    pub(crate) fn new(
        tracks: Vec<AudioTrack>,
        selected: Option<u32>,
        duration: Duration,
        playing: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            control: Mutex::new(Control {
                playing,
                stop: false,
                seek: None,
                hold: None,
                accurate: None,
                pixels: None,
                max_edge: 0,
                max_fps: 0.0,
                rate: 1.0,
                wants_audio: false,
                wants_previews: false,
                qualities: Vec::new(),
                pinned_quality: None,
                presented: Duration::ZERO,
                queue: VecDeque::new(),
                tracks,
                selected,
                duration,
                id: next_id(),
                pool_grows: 0,
                published: 0,
                anchor_epoch: 0,
                anchor_at: Duration::ZERO,
                manual_output: false,
                shown_pts: None,
                previews: Vec::new(),
                finished: false,
                renditions: Vec::new(),
                text_tracks: Vec::new(),
                selected_text: None,
                cues: Vec::new(),
                archive: Vec::new(),
                archive_bytes: 0,
            }),
            wake: Condvar::new(),
            pcm: Mutex::new(Pcm {
                live: false,
                muted: false,
                channels: 0,
                rate: 0,
                speed: 1.0,
                gain: 1.0,
                samples: VecDeque::new(),
                played: 0,
                anchor: Duration::ZERO,
                started: false,
                credit: 0.0,
                held: Vec::new(),
            }),
            speaker: Mutex::new(None),
        })
    }

    pub(crate) fn enable_audio(&self) {
        self.control.lock().expect("video").wants_audio = true;
    }

    pub(crate) fn manual_output(&self) -> bool {
        self.control.lock().expect("video").manual_output
    }

    /// Prepares the sample buffer and skips the output device. The caller pumps
    /// [`render_audio`](Self::render_audio) at the device rate.
    pub(crate) fn arm_manual_output(&self, channels: u16, rate: u32) {
        self.prepare_audio(channels, rate);
        self.control.lock().expect("video").manual_output = true;
    }

    pub(crate) fn enable_previews(&self) {
        self.control.lock().expect("video").wants_previews = true;
    }

    pub(crate) fn wants_audio(&self) -> bool {
        self.control.lock().expect("video").wants_audio
    }

    pub(crate) fn wants_previews(&self) -> bool {
        self.control.lock().expect("video").wants_previews
    }

    pub(crate) fn stopped(&self) -> bool {
        self.control.lock().expect("video").stop
    }

    pub(crate) fn playing(&self) -> bool {
        self.control.lock().expect("video").playing
    }

    pub(crate) fn seek_pending(&self) -> bool {
        let control = self.control.lock().expect("video");
        control.seek.is_some() || control.accurate.is_some()
    }

    pub(crate) fn clear_accurate(&self) {
        self.control.lock().expect("video").accurate = None;
    }

    pub(crate) fn pixel_size(&self) -> Option<(u32, u32)> {
        self.control.lock().expect("video").pixels
    }

    pub(crate) fn queue_full(&self) -> bool {
        self.control.lock().expect("video").queue.len() >= 2
    }

    pub(crate) fn max_edge(&self) -> u32 {
        self.control.lock().expect("video").max_edge
    }

    pub(crate) fn rate(&self) -> f32 {
        self.control.lock().expect("video").rate
    }

    pub(crate) fn set_rate(&self, rate: f32) {
        let rate = if rate.is_finite() {
            rate.clamp(0.25, 4.0)
        } else {
            1.0
        };
        self.control.lock().expect("video").rate = rate;
        if let Ok(mut pcm) = self.pcm.lock() {
            pcm.speed = rate;
        }
    }

    pub(crate) fn prepare_audio(&self, channels: u16, rate: u32) {
        let mut pcm = self.pcm.lock().expect("video");
        pcm.live = channels > 0 && rate > 0;
        pcm.channels = channels;
        pcm.rate = rate;
    }

    pub(crate) fn store_speaker(&self, commands: Sender<SpeakerCmd>) {
        *self.speaker.lock().expect("video") = Some(commands);
    }

    pub(crate) fn audio_enabled(&self) -> bool {
        self.pcm.lock().expect("video").live
    }

    pub(crate) fn audio_format(&self) -> Option<(u16, u32)> {
        let pcm = self.pcm.lock().expect("video");
        pcm.live.then_some((pcm.channels, pcm.rate))
    }

    pub(crate) fn anchor_at(&self) -> Duration {
        self.control.lock().expect("video").anchor_at
    }

    /// Samples have started and the device buffer is empty. Holding the picture here
    /// keeps the queue full, so the worker never reads the audio that would refill it.
    pub(crate) fn audio_starved(&self) -> bool {
        let pcm = self.pcm.lock().expect("video");
        pcm.live && !pcm.muted && pcm.started && pcm.samples.is_empty()
    }

    /// Pace on the picture queue only while the device still has samples.
    /// An empty buffer must not pin the decoder on the opening frames.
    pub(crate) fn holds_picture_queue(&self) -> bool {
        let pcm = self.pcm.lock().expect("video");
        if !pcm.live {
            return true;
        }
        !pcm.samples.is_empty()
    }

    /// Newest picture already decoded. Empty until the first frame is published.
    pub(crate) fn latest_pts(&self) -> Option<Duration> {
        let control = self.control.lock().expect("video");
        let queued = control.queue.back().map(|frame| frame.pts);
        let archived = control.archive.last().map(|(pts, _)| *pts);
        match (queued, archived) {
            (Some(queued), Some(archived)) => Some(queued.max(archived)),
            (Some(queued), None) => Some(queued),
            (None, Some(archived)) => Some(archived),
            (None, None) => None,
        }
    }

    pub(crate) fn selected_audio(&self) -> Option<u32> {
        self.control.lock().expect("video").selected
    }

    pub(crate) fn play_speaker(&self) {
        if self.muted() {
            return;
        }
        self.send_speaker(SpeakerCmd::Play);
    }

    pub(crate) fn pause_speaker(&self) {
        self.send_speaker(SpeakerCmd::Pause);
    }

    fn send_speaker(&self, command: SpeakerCmd) {
        if let Some(commands) = self.speaker.lock().expect("video").as_ref() {
            let _ = commands.send(command);
        }
    }

    pub(crate) fn muted(&self) -> bool {
        self.pcm.lock().expect("video").muted
    }

    pub(crate) fn volume(&self) -> f32 {
        self.pcm.lock().expect("video").gain
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        let gain = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.pcm.lock().expect("video").gain = gain;
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        self.pcm.lock().expect("video").muted = muted;
        if muted {
            self.pause_speaker();
        } else if self.playing() {
            self.play_speaker();
        }
    }

    pub(crate) fn audio_live(&self) -> bool {
        let pcm = self.pcm.lock().expect("video");
        pcm.live && !pcm.muted
    }

    pub(crate) fn pcm_full(&self) -> bool {
        let pcm = self.pcm.lock().expect("video");
        if !pcm.live || pcm.muted {
            return false;
        }
        let cap = pcm.rate as usize * usize::from(pcm.channels) * 2;
        pcm.samples.len() >= cap
    }

    pub(crate) fn push_pcm(&self, samples: &[f32]) {
        let mut pcm = self.pcm.lock().expect("video");
        if !pcm.live || pcm.muted || samples.is_empty() {
            return;
        }
        let cap = pcm.rate as usize * usize::from(pcm.channels) * 2;
        if pcm.samples.len() >= cap {
            return;
        }
        let room = cap - pcm.samples.len();
        pcm.samples.extend(samples.iter().take(room).copied());
    }

    pub(crate) fn clear_pcm(&self, anchor: Duration) {
        let mut pcm = self.pcm.lock().expect("video");
        pcm.samples.clear();
        pcm.played = 0;
        pcm.anchor = anchor;
        pcm.started = false;
        pcm.credit = 0.0;
        pcm.held.clear();
    }

    /// Samples already handed to the device, counted from the last seek.
    pub(crate) fn audio_time(&self) -> Option<Duration> {
        let pcm = self.pcm.lock().expect("video");
        if !pcm.started || pcm.muted || pcm.rate == 0 {
            return None;
        }
        Some(pcm.anchor + Duration::from_secs_f64(pcm.played as f64 / f64::from(pcm.rate)))
    }

    pub(crate) fn render_audio(&self, out: &mut [f32]) {
        // Catch-up frames are still hidden. Consuming now would play sound the picture has not reached.
        if self
            .control
            .try_lock()
            .is_ok_and(|control| control.accurate.is_some())
        {
            out.fill(0.0);
            return;
        }
        let Ok(mut pcm) = self.pcm.try_lock() else {
            out.fill(0.0);
            return;
        };
        if pcm.muted || pcm.channels == 0 {
            out.fill(0.0);
            return;
        }
        let channels = usize::from(pcm.channels);
        let speed = pcm.speed;
        let gain = pcm.gain;
        let mut consumed = 0_u64;
        for frame in out.chunks_mut(channels) {
            pcm.credit += speed;
            while pcm.credit >= 1.0 {
                let mut next = [0.0_f32; 2];
                let width = channels.min(next.len());
                if !take_frame(&mut pcm.samples, &mut next[..width]) {
                    // An empty buffer must not bank a debt. The next samples would be skipped.
                    pcm.credit = 0.0;
                    break;
                }
                pcm.held.clear();
                pcm.held.extend_from_slice(&next[..width]);
                pcm.credit -= 1.0;
                consumed += 1;
            }
            if pcm.held.len() == frame.len() {
                for (slot, sample) in frame.iter_mut().zip(pcm.held.iter()) {
                    *slot = sample * gain;
                }
            } else {
                frame.fill(0.0);
            }
        }
        if consumed > 0 {
            pcm.played += consumed;
            pcm.started = true;
            drop(pcm);
            self.notify();
        }
    }

    pub(crate) fn set_qualities(&self, qualities: Vec<VideoQuality>) {
        let mut kept = Vec::new();
        for quality in qualities {
            if quality.edge == 0 {
                continue;
            }
            if kept
                .iter()
                .any(|item: &VideoQuality| item.edge == quality.edge)
            {
                continue;
            }
            kept.push(quality);
        }
        self.control.lock().expect("video").qualities = kept;
    }

    pub(crate) fn qualities(&self) -> Vec<VideoQuality> {
        self.control.lock().expect("video").qualities.clone()
    }

    pub(crate) fn pinned_quality(&self) -> Option<u32> {
        self.control.lock().expect("video").pinned_quality
    }

    /// `None` clears the pin. An edge that is not in the list leaves the current pin.
    pub(crate) fn pin_quality(&self, edge: Option<u32>) {
        let mut control = self.control.lock().expect("video");
        match edge {
            None => control.pinned_quality = None,
            Some(edge) if control.qualities.iter().any(|quality| quality.edge == edge) => {
                control.pinned_quality = Some(edge);
            }
            Some(_) => {}
        }
    }

    pub(crate) fn take_seek(&self) -> Option<Duration> {
        self.control.lock().expect("video").seek.take()
    }

    pub(crate) fn wait(&self) {
        let control = self.control.lock().expect("video");
        let _guard = self.wake.wait(control).expect("video");
    }

    pub(crate) fn wait_timeout(&self, timeout: Duration) {
        let control = self.control.lock().expect("video");
        let _ = self.wake.wait_timeout(control, timeout);
    }

    pub(crate) fn set_duration(&self, duration: Duration) {
        self.control.lock().expect("video").duration = duration;
    }

    pub(crate) fn set_renditions(&self, renditions: Vec<(u32, Vec<String>)>) {
        self.control.lock().expect("video").renditions = renditions;
    }

    pub(crate) fn set_text(&self, tracks: Vec<TextTrack>, cues: Vec<(u32, Vec<TextCue>)>) {
        let mut control = self.control.lock().expect("video");
        control.selected_text = tracks.first().map(|track| track.id);
        control.text_tracks = tracks;
        control.cues = cues;
    }

    pub(crate) fn cue_at(&self, at: Duration) -> Option<String> {
        let control = self.control.lock().expect("video");
        let id = control.selected_text?;
        let cues = control.cues.iter().find(|(track, _)| *track == id)?;
        cues.1
            .iter()
            .find(|cue| cue.start <= at && at < cue.end)
            .map(|cue| cue.text.clone())
    }

    pub(crate) fn notify(&self) {
        self.wake.notify_all();
    }

    pub(crate) fn mark_finished(&self) {
        self.control.lock().expect("video").finished = true;
    }

    pub(crate) fn store_preview(&self, frame: FrameOut) {
        let mut control = self.control.lock().expect("video");
        if control.previews.iter().any(|(pts, _)| *pts == frame.pts) {
            return;
        }
        control.previews.push((frame.pts, frame.planes));
        control.previews.sort_by_key(|(pts, _)| *pts);
    }

    pub(crate) fn publish(
        &self,
        frame: FrameOut,
        grows: u32,
        last_published: &mut Option<Duration>,
    ) {
        let mut control = self.control.lock().expect("video");
        if control.seek.is_some() {
            return;
        }
        if control
            .accurate
            .is_some_and(|target| frame.pts + ACCURATE < target)
        {
            return;
        }
        let max_fps = control.max_fps;
        let skip = max_fps > 0.0
            && last_published.is_some_and(|previous| {
                frame.pts.saturating_sub(previous) < Duration::from_secs_f32(1.0 / max_fps)
            });
        if skip {
            return;
        }
        control.accurate = None;
        control.pixels = Some((frame.planes.width, frame.planes.height));
        control.pool_grows = grows;
        control.published += 1;
        let generation = control.published;
        let pts = frame.pts;
        let picture = VideoPicture {
            generation,
            pts,
            planes: frame.planes,
        };
        remember(&mut control, pts, &picture.planes);
        if let Some(existing) = control.queue.iter_mut().find(|queued| queued.pts == pts) {
            *existing = picture;
        } else {
            control.queue.push_back(picture);
        }
        while control.queue.len() > 2 {
            control.queue.pop_front();
        }
        *last_published = Some(pts);
        self.wake.notify_all();
    }
}

fn take_frame(samples: &mut VecDeque<f32>, frame: &mut [f32]) -> bool {
    if samples.len() < frame.len() {
        return false;
    }
    for sample in frame {
        *sample = samples.pop_front().unwrap_or(0.0);
    }
    true
}

pub(crate) fn quality_label(short_edge: u32) -> String {
    if short_edge == 0 {
        "Source".to_owned()
    } else {
        format!("{short_edge}p")
    }
}

/// `(long edge, short edge)` from a `WIDTHxHEIGHT` token.
pub(crate) fn resolution(value: &str) -> (u32, u32) {
    let dims: Vec<u32> = value
        .split(['x', 'X'])
        .filter_map(|part| part.parse().ok())
        .collect();
    let long = dims.iter().copied().max().unwrap_or(0);
    let short = dims.iter().copied().min().unwrap_or(long);
    (long, short)
}

const ARCHIVE_FINE: Duration = Duration::from_millis(33);
const ARCHIVE_COARSE: Duration = Duration::from_millis(400);
const ARCHIVE_REACH: Duration = Duration::from_millis(220);
const ACCURATE: Duration = Duration::from_millis(80);
const ARCHIVE_BYTES: usize = 96 * 1024 * 1024;

fn remember(control: &mut Control, pts: Duration, planes: &YuvPlanes) {
    let bytes = plane_bytes(planes);
    if let Some((_, stored)) = control
        .archive
        .iter_mut()
        .find(|(previous, _)| *previous == pts)
    {
        control.archive_bytes = control
            .archive_bytes
            .saturating_sub(plane_bytes(stored))
            .saturating_add(bytes);
        *stored = planes.clone();
        return;
    }
    if control
        .archive
        .last()
        .is_some_and(|(previous, _)| pts.saturating_sub(*previous) < ARCHIVE_FINE)
    {
        return;
    }
    control.archive.push((pts, planes.clone()));
    control.archive_bytes = control.archive_bytes.saturating_add(bytes);
    evict_archive(control);
}

/// Drops the oldest frame that sits inside a coarse gap, so watched media stays seekable.
fn evict_archive(control: &mut Control) {
    while control.archive_bytes > ARCHIVE_BYTES && control.archive.len() > 1 {
        let redundant = (1..control.archive.len() - 1).find(|&index| {
            let previous = control.archive[index - 1].0;
            let next = control.archive[index + 1].0;
            next.saturating_sub(previous) < ARCHIVE_COARSE
        });
        let index = redundant.unwrap_or(0);
        let (_, removed) = control.archive.remove(index);
        control.archive_bytes = control.archive_bytes.saturating_sub(plane_bytes(&removed));
    }
}

fn closest_archived(control: &Control, at: Duration) -> Option<(Duration, &YuvPlanes)> {
    let before = control.archive.iter().rev().find(|(pts, _)| *pts <= at);
    let after = control.archive.iter().find(|(pts, _)| *pts > at);
    let entry = match (before, after) {
        (Some(before), Some(after))
            if at.saturating_sub(before.0) <= after.0.saturating_sub(at) =>
        {
            before
        }
        (Some(_), Some(after)) => after,
        (Some(before), None) => before,
        (None, Some(after)) => after,
        (None, None) => return None,
    };
    Some((entry.0, &entry.1))
}

/// Media time of the stored picture closest to `at`, when one is close enough to show.
pub(crate) fn nearest_pts(control: &Control, at: Duration) -> Option<Duration> {
    let (pts, _) = closest_archived(control, at)?;
    let distance = if pts <= at {
        at.saturating_sub(pts)
    } else {
        pts.saturating_sub(at)
    };
    (distance <= ARCHIVE_REACH).then_some(pts)
}

/// The next picture the host should wake for. The live queue holds two frames and can sit
/// on the newest ones, so a queued time alone would skip pictures already stored.
pub(crate) fn soonest_picture(control: &Control, at: Duration) -> Option<Duration> {
    let queued = control
        .queue
        .iter()
        .filter(|frame| frame.pts > at)
        .map(|frame| frame.pts)
        .min();
    let stored = control
        .archive
        .iter()
        .filter(|(pts, _)| *pts > at)
        .map(|(pts, _)| *pts)
        .min();
    queued.into_iter().chain(stored).min()
}

/// A decoded picture close to `at`. Far seeks stay empty until that media is decoded again.
pub(crate) fn archived(control: &Control, at: Duration) -> Option<VideoPicture> {
    let (pts, planes) = closest_archived(control, at)?;
    if nearest_pts(control, at).is_none() {
        return None;
    }
    Some(VideoPicture {
        generation: archive_generation(pts),
        pts,
        planes: planes.clone(),
    })
}

fn archive_generation(pts: Duration) -> u64 {
    let millis = u64::try_from(pts.as_millis()).unwrap_or(u64::MAX);
    (1 << 63) | millis | 1
}

fn plane_bytes(planes: &YuvPlanes) -> usize {
    planes.y.len() + planes.u.len() + planes.v.len()
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_live_reads_the_pcm_state_without_locking_twice() {
        let session = Session::new(Vec::new(), None, Duration::ZERO, true);
        assert!(!session.audio_live());
        session.prepare_audio(2, 48_000);
        assert!(session.audio_live());
        session.set_muted(true);
        assert!(!session.audio_live());
    }

    #[test]
    fn volume_scales_the_samples_and_stays_inside_the_range() {
        let session = Session::new(Vec::new(), None, Duration::ZERO, true);
        session.prepare_audio(1, 48_000);
        session.push_pcm(&[1.0, 1.0, 1.0, 1.0]);
        session.set_volume(0.5);
        let mut out = [0.0; 4];
        session.render_audio(&mut out);
        assert!(out.iter().all(|sample| (*sample - 0.5).abs() < 0.001));
        session.set_volume(4.0);
        assert!((session.volume() - 1.0).abs() < 0.001);
        session.set_volume(f32::NAN);
        assert_eq!(session.volume(), 0.0);
    }

    #[test]
    fn an_underrun_does_not_skip_the_samples_that_arrive_later() {
        let session = Session::new(Vec::new(), None, Duration::ZERO, true);
        session.prepare_audio(1, 48_000);
        let mut silent = [0.0; 4_800];
        session.render_audio(&mut silent);
        session.push_pcm(&[0.25, 0.5, 0.75, 1.0]);
        let mut out = [0.0; 4];
        session.render_audio(&mut out);
        assert!(
            (out[0] - 0.25).abs() < 0.001,
            "the first sample after the gap was {out:?}"
        );
    }

    #[test]
    fn catching_up_hides_frames_until_the_seek_target() {
        use crate::decode::FrameOut;

        let session = Session::new(Vec::new(), None, Duration::from_secs(10), true);
        let mut last = None;
        session.publish(
            FrameOut {
                pts: Duration::ZERO,
                key: true,
                planes: YuvPlanes::solid(4, 4, 0, 128, 128),
            },
            0,
            &mut last,
        );
        session.publish(
            FrameOut {
                pts: Duration::from_secs(1),
                key: false,
                planes: YuvPlanes::solid(4, 4, 200, 128, 128),
            },
            0,
            &mut last,
        );
        session.control.lock().expect("video").accurate = Some(Duration::from_secs(1));
        session.publish(
            FrameOut {
                pts: Duration::from_millis(200),
                key: false,
                planes: YuvPlanes::solid(4, 4, 40, 128, 128),
            },
            0,
            &mut last,
        );
        let control = session.control.lock().expect("video");
        assert!(control.accurate.is_some());
        assert!(
            control
                .queue
                .iter()
                .all(|frame| frame.pts != Duration::from_millis(200))
        );
        assert_eq!(control.pixels, Some((4, 4)));
        drop(control);
        session.publish(
            FrameOut {
                pts: Duration::from_secs(1),
                key: false,
                planes: YuvPlanes::solid(4, 4, 200, 128, 128),
            },
            0,
            &mut last,
        );
        assert!(session.control.lock().expect("video").accurate.is_none());
    }
}

pub(crate) fn black_picture() -> VideoPicture {
    VideoPicture {
        generation: 0,
        pts: Duration::ZERO,
        planes: YuvPlanes::solid(2, 2, 0, 128, 128),
    }
}
