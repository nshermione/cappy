//! File playback: the pool is reused, and every audio stream is listed.

use std::thread;
use std::time::{Duration, Instant};

use cappy_video::{open, sample_path};

#[test]
fn two_frames_reuse_the_pool_and_both_audio_tracks_are_listed() {
    let source = open(sample_path()).expect("sample");
    wait_until(&source, 2);

    let grows = source.pool_grows();
    let first = source.picture_at(Duration::ZERO);
    assert_eq!(first.planes.width, 64);
    assert_eq!(first.planes.height, 64);

    source.picture_at(Duration::from_millis(80));
    wait_until(&source, 3);
    assert_eq!(
        source.pool_grows(),
        grows,
        "the next picture reused the pool"
    );
    let later = source.picture_at(Duration::from_millis(80));
    assert_ne!(
        first.planes.y.as_ref(),
        later.planes.y.as_ref(),
        "a later picture is a different frame"
    );

    let tracks = source.audio_tracks();
    assert_eq!(tracks.len(), 2);
    assert!(tracks.iter().any(|track| track.language == "eng"));
    assert!(tracks.iter().any(|track| track.language == "vie"));
    assert!(tracks.iter().all(|track| track.sample_rate == 48_000));
    let selected = source.selected_audio().expect("default track");
    let other = tracks
        .iter()
        .find(|track| track.id != selected)
        .expect("second track");
    source.select_audio(other.id);
    assert_eq!(source.selected_audio(), Some(other.id));

    source.pause();
    assert!(!source.playing());
}

fn wait_until(source: &cappy_core::VideoSource, count: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while source.published() < count {
        assert!(
            Instant::now() < deadline,
            "decoder published {} of {count}",
            source.published()
        );
        thread::sleep(Duration::from_millis(10));
    }
}
