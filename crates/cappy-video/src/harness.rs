//! Plays a real file through the decode worker and the UI clock, with no window and no speaker.
//! Each step reads the picture the UI would paint and the samples the device would play.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use cappy_core::time::Instant;
use cappy_core::{Application, FrameInput, Primitive, StyleRefinement, Theme, Ui};

use crate::file::open_manual;
use crate::session::Session;

const VIEWPORT: FrameInput = FrameInput {
    physical_width: 320,
    physical_height: 180,
    scale_factor: 1.0,
};

/// One step of the picture the node painted and the samples the device pulled.
#[derive(Clone, Debug)]
pub(crate) struct Sample {
    pub presented: Duration,
    pub picture: Option<Duration>,
    pub audio: Option<Duration>,
    pub peak: f32,
    pub luma: u8,
    pub generation: u64,
}

/// The real player, stepped by the test. `step` pulls audio and paints. `step_picture` paints
/// while the device is silent, which is a stall.
pub(crate) struct Playback {
    source: cappy_core::VideoSource,
    session: Arc<Session>,
    ui: Ui,
    now: Instant,
    channels: u16,
    rate: u32,
}

impl Playback {
    pub(crate) fn from_parts(
        source: cappy_core::VideoSource,
        session: Arc<Session>,
    ) -> Result<Self, String> {
        Application::new();
        Self::assemble(source, session)
    }

    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        Application::new();
        let (source, session) = if path.extension().and_then(|ext| ext.to_str()) == Some("m3u8") {
            crate::hls::open_manual(path)?
        } else {
            open_manual(path)?
        };
        Self::assemble(source, session)
    }

    fn assemble(source: cappy_core::VideoSource, session: Arc<Session>) -> Result<Self, String> {
        let (channels, rate) = session
            .audio_format()
            .ok_or_else(|| "no manual output".to_owned())?;
        let ui = Ui::new(
            cappy_core::video(source.clone()).style(StyleRefinement::default().size(64.0)),
            Theme::light(),
        );
        Ok(Self {
            source,
            session,
            ui,
            now: Instant::now(),
            channels,
            rate,
        })
    }

    pub(crate) fn seek(&mut self, at: Duration) {
        self.source.seek(at);
    }

    /// One redraw the window would do. The host sleeps until [`Ui::frame_deadline`], so a late
    /// deadline paints a single picture across a long stretch of media.
    pub(crate) fn step_host(&mut self) -> Sample {
        let wait = self
            .ui
            .frame_deadline(self.now)
            .map(|due| due.saturating_duration_since(self.now))
            .filter(|wait| !wait.is_zero())
            .unwrap_or(Duration::from_millis(16));
        self.step(wait)
    }

    /// Device callback for `dt`, then one UI frame at the clock those samples produced.
    pub(crate) fn step(&mut self, dt: Duration) -> Sample {
        let peak = self.consume(dt);
        self.paint(dt, peak)
    }

    /// One UI frame with the device not pulling. Samples already queued stay queued.
    pub(crate) fn step_picture(&mut self, dt: Duration) -> Sample {
        self.paint(dt, 0.0)
    }

    pub(crate) fn play_for(&mut self, total: Duration, step: Duration) -> Vec<Sample> {
        self.collect(total, step, true)
    }

    pub(crate) fn stall_for(&mut self, total: Duration, step: Duration) -> Vec<Sample> {
        self.collect(total, step, false)
    }

    pub(crate) fn run_until(
        &mut self,
        step: Duration,
        timeout: Duration,
        mut ready: impl FnMut(&Sample) -> bool,
    ) -> Sample {
        let started = self.now;
        let mut last = self.step(step);
        while !ready(&last) {
            assert!(
                self.now.saturating_duration_since(started) < timeout,
                "timed out at {}",
                describe(&last)
            );
            last = self.step(step);
        }
        last
    }

    fn collect(&mut self, total: Duration, step: Duration, hear: bool) -> Vec<Sample> {
        let mut frames = Vec::new();
        let mut done = Duration::ZERO;
        while done < total {
            frames.push(if hear {
                self.step(step)
            } else {
                self.step_picture(step)
            });
            done += step;
        }
        frames
    }

    fn consume(&mut self, dt: Duration) -> f32 {
        let frames = (u128::from(self.rate) * dt.as_nanos()) / 1_000_000_000;
        let frames = usize::try_from(frames).unwrap_or(1).max(1);
        let mut samples = vec![0.0; frames * usize::from(self.channels)];
        self.session.render_audio(&mut samples);
        samples
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    }

    fn paint(&mut self, dt: Duration, peak: f32) -> Sample {
        self.now += dt;
        self.ui.update_at(VIEWPORT, self.now);
        let (luma, generation) = self
            .ui
            .display_list()
            .iter()
            .find_map(|primitive| match primitive {
                Primitive::Video(video) => {
                    Some((video.y.first().copied().unwrap_or(0), video.generation))
                }
                Primitive::Image(_)
                | Primitive::Quad(_)
                | Primitive::Shadow(_)
                | Primitive::Text(_)
                | Primitive::Vector(_) => None,
            })
            .unwrap_or((0, 0));
        let picture = self.session.control.lock().expect("video").shown_pts;
        Sample {
            presented: self.source.presented(),
            picture,
            audio: self.source.audio_time(),
            peak,
            luma,
            generation,
        }
    }
}

pub(crate) fn describe(sample: &Sample) -> String {
    format!(
        "presented {:?} picture {:?} audio {:?} peak {:.4} luma {} generation {}",
        sample.presented, sample.picture, sample.audio, sample.peak, sample.luma, sample.generation
    )
}

fn apart(left: Duration, right: Duration) -> Duration {
    if left >= right {
        left - right
    } else {
        right - left
    }
}

/// The label stays on the painted frame. Sound is checked separately when the clip has samples.
pub(crate) fn label_on_picture(sample: &Sample, limit: Duration) {
    let picture = sample
        .picture
        .unwrap_or_else(|| panic!("no picture {}", describe(sample)));
    assert!(
        apart(sample.presented, picture) <= limit,
        "label left the picture {}",
        describe(sample)
    );
}

/// The label, the painted frame, and the sample clock stay on one time.
pub(crate) fn clocks_together(sample: &Sample, limit: Duration) {
    label_on_picture(sample, limit);
    let picture = sample
        .picture
        .unwrap_or_else(|| panic!("no picture {}", describe(sample)));
    let audio = sample
        .audio
        .unwrap_or_else(|| panic!("no audio {}", describe(sample)));
    assert!(
        apart(audio, picture) <= limit,
        "sound left the picture {}",
        describe(sample)
    );
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Instant;

    use cappy_core::{ButtonState, InputEvent, IntoElement, Point, PointerButton, VideoSource};

    use super::*;
    use crate::sample_path;

    const STEP: Duration = Duration::from_millis(20);
    const LIMIT: Duration = Duration::from_millis(150);

    fn vod_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/hls/vod.m3u8")
    }

    fn hear(play: &mut Playback) -> Sample {
        play.run_until(STEP, Duration::from_secs(2), |sample| {
            sample.generation != 0 && sample.audio.is_some() && sample.peak > 0.01
        })
    }

    /// Every video packet, then the audio. The device buffer stays empty while the pictures
    /// decode, which is the burst inside an HLS segment.
    fn open_video_burst() -> Playback {
        let bunny = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../playground/scenes/assets/big_buck_bunny.mp4");
        let demuxed = crate::file::demux_media(&bunny, true).expect("bunny");
        let mut video = Vec::new();
        let mut audio = Vec::new();
        for packet in demuxed.packets {
            if packet.audio {
                audio.push(packet);
            } else {
                video.push(packet);
            }
        }
        video.append(&mut audio);
        let (source, session) = crate::file::play_ordered(
            demuxed.config,
            video,
            demuxed.audio,
            Duration::from_secs(12),
        )
        .expect("play");
        Playback::from_parts(source, session).expect("ui")
    }

    fn open_video_ahead() -> Playback {
        let bunny = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../playground/scenes/assets/big_buck_bunny.mp4");
        let demuxed = crate::file::demux_media(&bunny, true).expect("bunny");
        let mut video = Vec::new();
        let mut audio = Vec::new();
        for packet in demuxed.packets {
            if packet.audio {
                audio.push(packet);
            } else {
                video.push(packet);
            }
        }
        assert!(
            video.len() > 40 && audio.len() > 8,
            "video {} audio {}",
            video.len(),
            audio.len()
        );
        let mut packets = Vec::new();
        packets.extend(video.drain(..2));
        packets.extend(audio.drain(..3));
        // More video than the stage cap, so the queue fills and the rest of the audio sits unread.
        packets.extend(video.drain(..24));
        while !video.is_empty() || !audio.is_empty() {
            if !video.is_empty() {
                packets.push(video.remove(0));
            }
            if !audio.is_empty() {
                packets.push(audio.remove(0));
            }
        }
        let (source, session) = crate::file::play_ordered(
            demuxed.config,
            packets,
            demuxed.audio,
            Duration::from_secs(12),
        )
        .expect("play");
        Playback::from_parts(source, session).expect("ui")
    }

    /// Distinct pictures per second of picture time.
    fn painted_rate(pictures: &[Duration]) -> (f32, Duration) {
        let mut distinct = Vec::new();
        for pts in pictures {
            if distinct.last() != Some(pts) {
                distinct.push(*pts);
            }
        }
        if distinct.len() < 2 {
            return (0.0, Duration::ZERO);
        }
        let span = distinct[distinct.len() - 1].saturating_sub(distinct[0]);
        if span.is_zero() {
            return (0.0, Duration::ZERO);
        }
        ((distinct.len() - 1) as f32 / span.as_secs_f32(), span)
    }

    /// Shown second 0 used to stick: the label, the picture, and the samples never left it.
    #[test]
    fn playback_does_not_freeze_at_zero() {
        let mut play = Playback::open(&sample_path()).expect("sample");
        let started = hear(&mut play);
        clocks_together(&started, LIMIT);
        let playing = play.play_for(Duration::from_millis(200), STEP);
        let first = playing
            .first()
            .and_then(|sample| sample.picture)
            .unwrap_or_default();
        let last = playing.last().expect("steps");
        assert!(
            last.picture.expect("picture").saturating_sub(first) >= Duration::from_millis(80),
            "picture stayed at {first:?}: {}",
            describe(last)
        );
        assert!(
            last.presented.saturating_sub(first) >= Duration::from_millis(80),
            "clock stayed at {first:?}: {}",
            describe(last)
        );
        assert!(
            playing.iter().any(|sample| sample.peak > 0.01),
            "sound never started {}",
            describe(last)
        );
        for sample in &playing {
            if sample.audio.is_some() && sample.picture.is_some() {
                clocks_together(sample, LIMIT);
            }
        }
    }

    /// A segment with no audio track used to pin the clock at 0 because the device never ticked.
    #[test]
    fn hls_without_sound_still_leaves_zero() {
        let mut play = Playback::open(&vod_path()).expect("vod");
        let left = play.run_until(STEP, Duration::from_secs(2), |sample| {
            sample
                .picture
                .is_some_and(|pts| pts >= Duration::from_millis(100))
                && sample.presented >= Duration::from_millis(100)
        });
        label_on_picture(&left, LIMIT);
        let playing = play.play_for(Duration::from_millis(200), STEP);
        for sample in &playing {
            if sample.picture.is_some() {
                label_on_picture(sample, LIMIT);
            }
        }
        play.seek(Duration::ZERO);
        let back = play.run_until(STEP, Duration::from_secs(2), |sample| {
            sample.generation != 0
                && sample
                    .picture
                    .is_some_and(|pts| pts < Duration::from_millis(80))
        });
        label_on_picture(&back, LIMIT);
        let forward = play.run_until(STEP, Duration::from_secs(2), |sample| {
            sample
                .picture
                .is_some_and(|pts| pts >= Duration::from_millis(100))
                && sample.presented >= Duration::from_millis(100)
        });
        label_on_picture(&forward, LIMIT);
    }

    /// The label used to keep counting while the device was stalled and the picture did not.
    #[test]
    fn a_stall_does_not_run_the_clock_past_the_picture() {
        let mut play = Playback::open(&sample_path()).expect("sample");
        let _ = hear(&mut play);
        let _ = play.play_for(Duration::from_millis(100), STEP);
        let before = play.step(STEP);
        clocks_together(&before, LIMIT);
        let stalled = play.stall_for(Duration::from_millis(300), STEP);
        let held = stalled.last().expect("stall");
        clocks_together(held, LIMIT);
        assert!(
            held.presented.saturating_sub(before.presented) <= LIMIT,
            "clock kept running from {} while sound was stalled, now {}",
            describe(&before),
            describe(held)
        );
    }

    /// Seeking back to 0 used to leave both the picture and the sound stuck, with a dead clock.
    #[test]
    fn seeking_to_zero_restarts_picture_and_sound_together() {
        let mut play = Playback::open(&sample_path()).expect("sample");
        let _ = hear(&mut play);
        let _ = play.play_for(Duration::from_millis(200), STEP);
        play.seek(Duration::ZERO);
        let restarted = play.run_until(STEP, Duration::from_secs(2), |sample| {
            sample.audio.is_some()
                && sample.peak > 0.01
                && sample
                    .picture
                    .is_some_and(|pts| pts < Duration::from_millis(120))
        });
        clocks_together(&restarted, LIMIT);
        let again = play.play_for(Duration::from_millis(200), STEP);
        assert!(
            again.iter().any(|sample| sample.peak > 0.01),
            "sound did not return after the seek {}",
            again.last().map(describe).unwrap_or_default()
        );
        let end = again.last().expect("after seek");
        clocks_together(end, LIMIT);
        assert!(
            end.picture.expect("picture") >= Duration::from_millis(80),
            "picture did not run after the seek {}",
            describe(end)
        );
        assert!(
            end.presented >= Duration::from_millis(80),
            "clock did not run after the seek {}",
            describe(end)
        );
    }

    /// A few audio packets, then a run of video, then the rest of the audio.
    /// The picture queue fills, the short burst is consumed, and playback used to sit on 0:00.
    #[test]
    fn video_ahead_of_audio_does_not_freeze_at_zero() {
        let mut play = open_video_ahead();
        let moved = play.run_until(STEP, Duration::from_secs(3), |sample| {
            sample.presented >= Duration::from_millis(800)
                && sample
                    .picture
                    .is_some_and(|pts| pts >= Duration::from_millis(400))
                && sample.peak > 0.01
        });
        assert!(
            moved.presented >= Duration::from_millis(800),
            "clock stayed on the opening {}",
            describe(&moved)
        );
        assert!(
            moved
                .audio
                .is_some_and(|at| at >= Duration::from_millis(40)),
            "sound did not continue {}",
            describe(&moved)
        );
    }

    /// The window used to sleep until a frame the 2-deep queue had already run past,
    /// so a 24 fps clip painted in the mid-teens.
    #[test]
    fn the_window_paints_near_the_source_rate_when_video_runs_ahead() {
        let mut play = open_video_burst();
        let started = Instant::now();
        while play
            .session
            .latest_pts()
            .is_none_or(|pts| pts < Duration::from_secs(1))
        {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "pictures did not decode ahead of the clock"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut seen = Vec::new();
        let mut virtual_time = Duration::ZERO;
        let mut origin = None;
        while virtual_time < Duration::from_secs(4) {
            let before = play.now;
            let sample = play.step_host();
            virtual_time += play.now.saturating_duration_since(before);
            let Some(picture) = sample.picture else {
                continue;
            };
            let start = *origin.get_or_insert(picture);
            seen.push(picture);
            if picture.saturating_sub(start) >= Duration::from_millis(800) && seen.len() > 4 {
                break;
            }
        }
        let (fps, span) = painted_rate(&seen);
        assert!(fps >= 20.0, "window painted {fps:.1} fps across {span:?}");
        let mut previous = None;
        for pts in &seen {
            if let Some(before) = previous
                && *pts != before
            {
                assert!(
                    *pts > before,
                    "picture went back from {before:?} to {pts:?}"
                );
                let gap = pts.saturating_sub(before);
                assert!(
                    gap <= Duration::from_millis(50),
                    "picture jumped {gap:?} while painting {fps:.1} fps"
                );
            }
            previous = Some(*pts);
        }
    }

    /// Picture time and sample time used to walk apart.
    #[test]
    fn sound_does_not_drift_from_the_picture() {
        let mut play = Playback::open(&sample_path()).expect("sample");
        let _ = hear(&mut play);
        let playing = play.play_for(Duration::from_millis(300), STEP);
        assert!(
            playing.iter().any(|sample| sample.peak > 0.01),
            "the device buffer stayed silent"
        );
        let mut heard = 0;
        for sample in &playing {
            if sample.audio.is_some() && sample.picture.is_some() {
                clocks_together(sample, LIMIT);
                heard += 1;
            }
        }
        assert!(heard > 5, "sound was missing for most of the run");
    }

    const STAGE: FrameInput = FrameInput {
        physical_width: 800,
        physical_height: 600,
        scale_factor: 1.0,
    };

    struct PlayerChrome {
        ui: Ui,
        now: Instant,
        source: Rc<RefCell<Option<VideoSource>>>,
    }

    impl PlayerChrome {
        fn open() -> Self {
            Application::new();
            let source = Rc::new(RefCell::new(None));
            let slot = Rc::clone(&source);
            let bunny = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../playground/scenes/assets/big_buck_bunny.mp4");
            let ui = Ui::new(
                crate::player(bunny.to_string_lossy().into_owned())
                    .audio(false)
                    .on_ready(move |opened| *slot.borrow_mut() = Some(opened.clone()))
                    .into_element(),
                Theme::light(),
            );
            Self {
                ui,
                now: Instant::now(),
                source,
            }
        }

        fn step(&mut self, dt: Duration) {
            self.now += dt;
            self.ui.update_at(STAGE, self.now);
        }

        fn shown_pts(&self) -> Duration {
            let borrowed = self.source.borrow();
            let Some(source) = borrowed.as_ref() else {
                return Duration::ZERO;
            };
            let at = source.presented();
            source.picture_at(at).pts
        }

        fn presented(&self) -> Duration {
            self.source
                .borrow()
                .as_ref()
                .map(|source| source.presented())
                .unwrap_or_default()
        }

        fn wait_until_playing(&mut self) {
            let wall = Instant::now();
            while self.presented().is_zero() || self.ui.find_by_key("video-stage").is_none() {
                assert!(
                    wall.elapsed() < Duration::from_secs(3),
                    "the player did not open"
                );
                std::thread::sleep(Duration::from_millis(10));
                self.step(Duration::from_millis(50));
            }
        }

        /// Steps until `presented` has moved by `media`, so the wall clock and the chrome timer both run.
        fn advance(&mut self, media: Duration) {
            let start = self.presented();
            let wall = Instant::now();
            let mut virtual_time = Duration::ZERO;
            while self.presented().saturating_sub(start) < media {
                assert!(
                    wall.elapsed() < Duration::from_secs(4)
                        && virtual_time < Duration::from_secs(4),
                    "playback stuck at {:?} after {virtual_time:?}",
                    self.presented()
                );
                self.step(Duration::from_millis(50));
                virtual_time += Duration::from_millis(50);
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        fn reveal(&mut self) {
            let stage = self.ui.find_by_key("video-stage").expect("stage");
            let rect = self.ui.node_rect(stage).expect("stage rect");
            self.ui.handle_event(InputEvent::PointerMoved {
                position: Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5),
            });
            self.step(Duration::from_millis(20));
        }

        fn click(&mut self, key: &str) {
            let id = self
                .ui
                .find_by_key(key)
                .unwrap_or_else(|| panic!("missing {key}"));
            let rect = self.ui.node_rect(id).expect(key);
            let position = Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
            self.ui.handle_event(InputEvent::PointerMoved { position });
            self.ui.handle_event(InputEvent::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            });
            self.ui.handle_event(InputEvent::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Released,
            });
            self.step(Duration::from_millis(20));
        }

        fn clock(&mut self) -> String {
            self.reveal();
            let id = self.ui.find_by_key("video-clock").expect("clock");
            self.ui.node_text(id).unwrap_or("").to_owned()
        }

        fn playhead_x(&mut self) -> f32 {
            self.reveal();
            let id = self.ui.find_by_key("video-playhead").expect("playhead");
            self.ui.node_rect(id).expect("playhead rect").x
        }

        fn round_trip_fullscreen(&mut self) {
            self.reveal();
            self.click("video-full");
            self.reveal();
            self.click("video-full");
        }

        fn seek_fraction(&mut self, fraction: f32) {
            self.reveal();
            let bar = self.ui.find_by_key("video-progress").expect("progress");
            let rect = self.ui.node_rect(bar).expect("progress rect");
            let position = Point::new(
                rect.x + rect.width * fraction.clamp(0.0, 1.0),
                rect.y + rect.height * 0.5,
            );
            self.ui.handle_event(InputEvent::PointerMoved { position });
            self.ui.handle_event(InputEvent::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            });
            self.ui.handle_event(InputEvent::PointerButton {
                button: PointerButton::Primary,
                state: ButtonState::Released,
            });
            self.step(Duration::from_millis(20));
        }
    }

    fn clock_seconds(label: &str) -> u64 {
        let stamp = label.split(" / ").next().unwrap_or(label);
        let mut parts = stamp.split(':').collect::<Vec<_>>();
        let seconds = parts.pop().unwrap_or("0").parse::<u64>().unwrap_or(0);
        let minutes = parts.pop().unwrap_or("0").parse::<u64>().unwrap_or(0);
        let hours = parts.pop().unwrap_or("0").parse::<u64>().unwrap_or(0);
        hours * 3600 + minutes * 60 + seconds
    }

    /// Normal, fullscreen, then normal again used to drop the chrome timer and rebuild the
    /// picture at 0. The clock and the bar froze while the media kept going, and the next seek lagged.
    #[test]
    fn leaving_fullscreen_keeps_the_clock_the_bar_and_the_seek() {
        let mut play = PlayerChrome::open();
        play.wait_until_playing();
        play.advance(Duration::from_millis(1200));
        let before = play.presented();
        let label = play.clock();
        let playhead = play.playhead_x();
        assert!(
            clock_seconds(&label) >= 1,
            "clock did not leave 0 before fullscreen: {label} at {before:?}"
        );

        play.round_trip_fullscreen();
        let returned = play.presented();
        assert!(
            returned + Duration::from_millis(200) >= before,
            "picture jumped from {before:?} to {returned:?} after leaving fullscreen"
        );

        play.advance(Duration::from_millis(1200));
        let later = play.clock();
        let moved = play.playhead_x();
        assert!(
            clock_seconds(&later) > clock_seconds(&label),
            "clock froze at {label} after leaving fullscreen, now {later} presented {:?}",
            play.presented()
        );
        assert!(
            moved > playhead + 1.0,
            "progress stayed at {playhead} after leaving fullscreen, now {moved}"
        );

        let duration = play
            .source
            .borrow()
            .as_ref()
            .map(|source| source.duration())
            .unwrap_or_default();
        let target = duration.mul_f32(0.5);
        play.seek_fraction(0.5);
        let wall = Instant::now();
        let mut virtual_time = Duration::ZERO;
        while play.shown_pts().abs_diff(target) > Duration::from_millis(300) {
            assert!(
                wall.elapsed() < Duration::from_secs(3) && virtual_time < Duration::from_secs(2),
                "seek to {target:?} lagged, picture {:?} presented {:?} after {virtual_time:?}",
                play.shown_pts(),
                play.presented()
            );
            play.step(Duration::from_millis(50));
            virtual_time += Duration::from_millis(50);
            std::thread::sleep(Duration::from_millis(5));
        }
        play.advance(Duration::from_millis(1100));
        let after_seek = play.clock();
        assert!(
            clock_seconds(&after_seek) + 1 >= target.as_secs(),
            "clock {after_seek} did not follow the seek to {target:?}"
        );
    }
}
