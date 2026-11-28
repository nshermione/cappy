//! Maps a pointer position on the progress bar to a media time.

use std::time::Duration;

/// Fraction of `width` that `x` sits at, measured from `left`, as a media time.
pub fn scrub_pts(x: f32, left: f32, width: f32, duration: Duration) -> Duration {
    if width <= 0.0 || duration.is_zero() {
        return Duration::ZERO;
    }
    let fraction = ((x - left) / width).clamp(0.0, 1.0);
    duration.mul_f32(fraction)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scrub {
    Hover(Duration),
    Seek(Duration),
}

/// Hover and drag report [`Scrub::Hover`]. Releasing the pointer reports [`Scrub::Seek`].
pub fn scrub_action(release: bool, x: f32, left: f32, width: f32, duration: Duration) -> Scrub {
    let pts = scrub_pts(x, left, width, duration);
    if release {
        Scrub::Seek(pts)
    } else {
        Scrub::Hover(pts)
    }
}
