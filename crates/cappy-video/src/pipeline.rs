//! The packet list below is just bytes. Nothing in this test names FFmpeg.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use cappy_core::{VideoPicture, VideoSource};

use crate::dash::open_paused as open_dash_paused;
use crate::file::{load_packets, open, open_paused, play_packets, sample_path};
use crate::hls::open_paused as open_hls_paused;
use crate::playlist::{open_dash, open_hls};
use crate::scrub::{Scrub, scrub_action};

#[test]
fn a_packet_list_decodes_through_the_same_pool() {
    let (config, packets) = load_packets(&sample_path()).expect("packets");
    assert!(packets.len() > 2);
    let source =
        play_packets(config, packets, Vec::new(), Duration::from_millis(500)).expect("play");
    let deadline = Instant::now() + Duration::from_secs(5);
    while source.published() < 2 {
        assert!(
            Instant::now() < deadline,
            "published {}",
            source.published()
        );
        thread::sleep(Duration::from_millis(10));
    }
    let grows = source.pool_grows();
    let _ = source.picture_at(Duration::from_millis(80));
    while source.published() < 3 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(source.published() >= 3);
    assert_eq!(source.pool_grows(), grows);
}

#[test]
fn seek_across_a_gop_replaces_the_picture_and_keeps_the_audio_track() {
    let with_audio = open(sample_path()).expect("sample");
    let selected = with_audio.selected_audio();
    with_audio.seek(Duration::from_millis(200));
    assert_eq!(with_audio.selected_audio(), selected);

    let source = open(fixture("gop.mp4")).expect("gop");
    let opening = wait_picture(&source, Duration::ZERO, |picture| picture.generation > 0);
    source.seek(Duration::from_millis(700));
    let later = wait_picture(&source, Duration::from_millis(700), |picture| {
        picture.pts >= Duration::from_millis(450) && picture.planes.y != opening.planes.y
    });
    assert!(later.planes.width > 0);
    let early = source.picture_at(Duration::from_millis(80));
    assert!(
        early.generation == 0 || early.pts < later.pts,
        "a picture after the seek is not shown at the old time"
    );
}

#[test]
fn max_edge_scales_planes_and_max_fps_drops_pictures() {
    let scaled = open_paused(sample_path()).expect("sample");
    scaled.set_max_edge(32);
    scaled.play();
    let picture = wait_picture(&scaled, Duration::ZERO, |picture| picture.generation > 0);
    assert_eq!(picture.planes.width, 32);
    assert_eq!(picture.planes.height, 32);

    let paced = open_paused(fixture("four.mp4")).expect("four");
    paced.set_max_fps(2.0);
    paced.play();
    let deadline = Instant::now() + Duration::from_secs(5);
    while paced.published() < 2 {
        assert!(Instant::now() < deadline, "published {}", paced.published());
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(300));
    assert_eq!(paced.published(), 2);
}

#[test]
fn a_time_between_keyframes_shows_the_previous_preview() {
    let source = crate::open_with(
        fixture("gop.mp4"),
        crate::OpenOptions {
            audio: false,
            previews: true,
        },
    )
    .expect("gop");
    let deadline = Instant::now() + Duration::from_secs(5);
    let early = loop {
        if let Some(planes) = source.preview_at(Duration::from_millis(200)) {
            break planes;
        }
        assert!(Instant::now() < deadline, "no preview");
        thread::sleep(Duration::from_millis(10));
    };
    let between = source
        .preview_at(Duration::from_millis(700))
        .expect("later preview");
    let at_key = source
        .preview_at(Duration::from_millis(500))
        .expect("key preview");
    assert_eq!(between.y.as_ref(), at_key.y.as_ref());
    assert_ne!(between.y.as_ref(), early.y.as_ref());
    assert!(between.width <= 160 && between.height <= 160);
}

#[test]
fn releasing_the_bar_seeks_to_that_point() {
    let duration = Duration::from_secs(10);
    let action = scrub_action(true, 150.0, 0.0, 300.0, duration);
    assert_eq!(action, Scrub::Seek(Duration::from_secs(5)));
    let hover = scrub_action(false, 150.0, 0.0, 300.0, duration);
    assert_eq!(hover, Scrub::Hover(Duration::from_secs(5)));
}

#[test]
fn hls_vod_seeks_into_the_next_segment_and_lists_audio() {
    let vod = open_hls(fixture("hls/vod.m3u8")).expect("vod");
    let opening = wait_picture(&vod, Duration::ZERO, |picture| picture.generation > 0);
    assert!(
        average_luma(&opening.planes.y) < 40,
        "first segment is black"
    );
    vod.seek(Duration::from_millis(500));
    let later = wait_picture(&vod, Duration::from_millis(500), |picture| {
        average_luma(&picture.planes.y) > 180
    });
    assert!(later.planes.width > 0);

    let master = open_hls(fixture("hls/master.m3u8")).expect("master");
    let tracks = master.audio_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].language, "en");
    let segments = master.audio_segments(tracks[0].id);
    assert_eq!(segments.len(), 1);
    assert!(segments[0].ends_with("audio0.ts"));
    master.select_audio(tracks[0].id);
    assert_eq!(master.selected_audio(), Some(tracks[0].id));

    vod.seek(Duration::ZERO);
    let back = vod.picture_at(Duration::ZERO);
    assert_ne!(back.generation, 0, "the loaded start is still cached");
    assert!(
        average_luma(&back.planes.y) < 40,
        "seeking back shows the first segment without fetching it again"
    );
}

#[test]
fn dash_vod_seeks_into_the_next_segment_and_lists_audio() {
    let vod = open_dash(fixture("dash/vod.mpd")).expect("dash");
    let opening = wait_picture(&vod, Duration::ZERO, |picture| picture.generation > 0);
    assert!(
        average_luma(&opening.planes.y) < 40,
        "first segment is black"
    );
    vod.seek(Duration::from_millis(500));
    let later = wait_picture(&vod, Duration::from_millis(500), |picture| {
        average_luma(&picture.planes.y) > 180
    });
    assert!(later.planes.width > 0);
    let tracks = vod.audio_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].language, "en");
    let segments = vod.audio_segments(tracks[0].id);
    assert_eq!(segments.len(), 1);
    assert!(segments[0].ends_with("audio0.ts"), "{}", segments[0]);

    let capped = open_dash_paused(fixture("dash/vod.mpd")).expect("capped");
    capped.set_max_edge(32);
    capped.play();
    let picture = wait_picture(&capped, Duration::ZERO, |picture| picture.generation > 0);
    assert_eq!(picture.planes.width, 32);
}

#[test]
fn hls_live_edge_advances_when_a_segment_is_appended() {
    let dir = std::env::temp_dir().join(format!("cappy-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let playlist = dir.join("live.m3u8");
    let seg0 = fixture("hls/seg0.ts");
    let seg1 = fixture("hls/seg1.ts");
    std::fs::write(
        &playlist,
        format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:0.4,\n{}\n",
            seg0.display()
        ),
    )
    .unwrap();
    let source = open_hls(&playlist).expect("live");
    let _ = wait_picture(&source, Duration::ZERO, |picture| picture.generation > 0);
    assert!(source.duration() < Duration::from_millis(500));
    std::fs::write(
        &playlist,
        format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:0.4,\n{}\n#EXTINF:0.4,\n{}\n",
            seg0.display(),
            seg1.display()
        ),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while source.duration() < Duration::from_millis(700) {
        let _ = source.picture_at(source.duration());
        assert!(
            Instant::now() < deadline,
            "live edge stayed {}",
            source.duration().as_secs_f32()
        );
        thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_slow_fetch_steps_down_and_max_edge_skips_the_large_variant() {
    let slow = open_hls_paused(fixture("hls/abr.m3u8")).expect("abr");
    slow.play();
    let first = wait_picture(&slow, Duration::ZERO, |picture| picture.planes.width == 64);
    assert_eq!(first.planes.width, 64);
    let second = wait_picture(&slow, Duration::from_millis(500), |picture| {
        picture.planes.width == 32
    });
    assert_eq!(second.planes.width, 32);

    let capped = open_hls_paused(fixture("hls/cap.m3u8")).expect("cap");
    capped.set_max_edge(32);
    capped.play();
    let picture = wait_picture(&capped, Duration::ZERO, |picture| picture.generation > 0);
    assert_eq!(picture.planes.width, 32);
}

#[test]
fn encrypted_segments_match_the_clear_pictures() {
    let clear = open_hls(fixture("hls/vod.m3u8")).expect("clear");
    let encrypted = open_hls(fixture("hls/encrypted.m3u8")).expect("encrypted");
    let clear_open = wait_picture(&clear, Duration::ZERO, |picture| picture.generation > 0);
    let hidden_open = wait_picture(&encrypted, Duration::ZERO, |picture| picture.generation > 0);
    assert_eq!(
        average_luma(&clear_open.planes.y),
        average_luma(&hidden_open.planes.y)
    );
    clear.seek(Duration::from_millis(500));
    encrypted.seek(Duration::from_millis(500));
    let clear_later = wait_picture(&clear, Duration::from_millis(500), |picture| {
        average_luma(&picture.planes.y) > 180
    });
    let hidden_later = wait_picture(&encrypted, Duration::from_millis(500), |picture| {
        average_luma(&picture.planes.y) > 180
    });
    assert_eq!(
        average_luma(&clear_later.planes.y),
        average_luma(&hidden_later.planes.y)
    );
    let error = match open_hls(fixture("hls/sample-aes.m3u8")) {
        Ok(_) => panic!("sample-aes should fail"),
        Err(error) => error,
    };
    assert!(error.contains("SAMPLE-AES"), "{error}");
}

#[test]
fn a_part_is_shown_before_its_parent_segment_exists() {
    let source = open_hls(fixture("hls/parts.m3u8")).expect("parts");
    let first = wait_picture(&source, Duration::ZERO, |picture| picture.generation > 0);
    assert!(
        average_luma(&first.planes.y) < 40,
        "the first part is the black picture"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        source.picture_at(Duration::from_secs(2));
        let picture = source.picture_at(Duration::ZERO);
        if average_luma(&picture.planes.y) > 180 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "part replacement did not arrive, published {}",
            source.published()
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(source.published() >= 2, "the later part replaced the first");
}

#[test]
fn cues_change_at_the_boundary_and_an_unselected_track_is_silent() {
    let hls = open_hls(fixture("hls/subs.m3u8")).expect("hls subs");
    assert_cue_boundary(&hls);
    let dash = open_dash(fixture("dash/vod.mpd")).expect("dash subs");
    assert_cue_boundary(&dash);
    let container = crate::vtt::from_container(&fixture("subs/en.vtt"));
    assert!(
        container.tracks.is_empty() || container.cues.iter().any(|(_, cues)| cues.len() == 2),
        "a container subtitle stream yields its cues"
    );
}

fn assert_cue_boundary(source: &VideoSource) {
    let tracks = source.text_tracks();
    assert_eq!(tracks.len(), 2, "{tracks:?}");
    assert_eq!(source.cue_at(Duration::ZERO).as_deref(), Some("black"));
    source.seek(Duration::from_millis(400));
    assert_eq!(
        source.cue_at(Duration::from_millis(400)).as_deref(),
        Some("white")
    );
    source.select_text(tracks[1].id);
    assert_eq!(source.selected_text(), Some(tracks[1].id));
    assert_eq!(source.cue_at(Duration::ZERO), None);
}

#[test]
fn asking_for_audio_opens_the_clip_and_a_picture_open_stays_silent() {
    let quiet = open(sample_path()).expect("quiet");
    assert!(!quiet.wants_audio());
    assert!(!quiet.audio_enabled());

    let heard = crate::open_with(
        sample_path(),
        crate::OpenOptions {
            audio: true,
            previews: false,
        },
    )
    .expect("heard");
    assert!(heard.wants_audio());
    if !heard.audio_tracks().is_empty() && heard.audio_enabled() {
        let deadline = Instant::now() + Duration::from_secs(5);
        while heard.audio_time().is_none() {
            assert!(Instant::now() < deadline, "the device produced no samples");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn playback_rate_is_kept_and_a_pinned_rung_is_the_one_that_plays() {
    let file = open(sample_path()).expect("file");
    assert_eq!(file.playback_rate(), 1.0);
    file.set_playback_rate(1.5);
    assert!((file.playback_rate() - 1.5).abs() < 0.001);
    file.set_playback_rate(0.0);
    assert!((file.playback_rate() - 0.25).abs() < 0.001);
    assert!(file.qualities().is_empty());

    let hls = open_hls_paused(fixture("hls/cap.m3u8")).expect("cap");
    let labels: Vec<_> = hls
        .qualities()
        .into_iter()
        .map(|quality| quality.label)
        .collect();
    assert_eq!(labels, ["64p", "32p"]);
    assert_eq!(hls.selected_quality(), None);
    hls.select_quality(Some(1));
    assert_eq!(hls.selected_quality(), None);
    hls.select_quality(Some(32));
    assert_eq!(hls.selected_quality(), Some(32));
    hls.play();
    let picture = wait_picture(&hls, Duration::ZERO, |picture| picture.generation > 0);
    assert_eq!(picture.planes.width, 32);
}

fn average_luma(samples: &[u8]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let sum: u32 = samples.iter().map(|sample| u32::from(*sample)).sum();
    sum / samples.len() as u32
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn wait_picture(
    source: &VideoSource,
    at: Duration,
    ready: impl Fn(&VideoPicture) -> bool,
) -> VideoPicture {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let picture = source.picture_at(at);
        if ready(&picture) {
            return picture;
        }
        assert!(
            Instant::now() < deadline,
            "picture at {at:?} did not arrive"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
