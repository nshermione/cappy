//! Timing for the debug stats signal. Sampling itself stays in the window host, and only runs
//! while a binding is subscribed.

use cappy_core::time::{Duration, Instant};

/// How long after the last real present the host wakes once so an idle panel can show 0 fps.
pub const OVERLAY_IDLE: Duration = Duration::from_secs(1);

/// What the latest counted paint pass submitted. A later paint that is not counted keeps this,
/// so the panel still describes the last real repaint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintStats {
    pub batches: usize,
    pub draw_calls: usize,
    pub regions: usize,
    /// Share of the viewport covered by the scissors, clamped to 100.
    pub coverage_percent: u32,
}

/// `covered_px / viewport_px`, as a percent, never above 100. Overlapping scissors can sum past
/// the viewport; the panel caps that instead of printing a nonsense ratio.
pub fn coverage_percent(covered_px: u64, viewport_px: u64) -> u32 {
    if viewport_px == 0 {
        return 0;
    }
    let percent = covered_px.saturating_mul(100) / viewport_px;
    u32::try_from(percent.min(100)).unwrap_or(100)
}

/// Presents in the trailing second. A stats refresh passes `record = false`, so it drops out of
/// the window without adding itself.
pub fn rolling_fps(samples: &mut Vec<Instant>, now: Instant, record: bool) -> u32 {
    if record {
        samples.push(now);
    }
    samples.retain(|at| now.saturating_duration_since(*at) < OVERLAY_IDLE);
    u32::try_from(samples.len()).unwrap_or(u32::MAX)
}

/// The idle stats wake is a refresh only when the UI itself had nothing to paint.
pub fn is_overlay_refresh(refresh_due: bool, ui_due: bool, ui_dirty: bool) -> bool {
    refresh_due && !ui_due && !ui_dirty
}

/// The sooner of the UI deadline and the idle stats wake.
pub fn earlier(first: Option<Instant>, second: Option<Instant>) -> Option<Instant> {
    match (first, second) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(instant), None) | (None, Some(instant)) => Some(instant),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_clamps_and_treats_an_empty_viewport_as_zero() {
        assert_eq!(coverage_percent(4, 100), 4);
        assert_eq!(coverage_percent(150, 100), 100);
        assert_eq!(coverage_percent(1, 0), 0);
    }

    #[test]
    fn fps_window_drops_samples_older_than_a_second_and_ignores_a_refresh() {
        let start = Instant::now();
        let mut samples = Vec::new();
        assert_eq!(rolling_fps(&mut samples, start, true), 1);
        assert_eq!(
            rolling_fps(&mut samples, start + Duration::from_millis(500), true),
            2
        );
        assert_eq!(
            rolling_fps(&mut samples, start + Duration::from_millis(900), false),
            2
        );
        assert_eq!(
            rolling_fps(&mut samples, start + Duration::from_millis(1500), false),
            0
        );
    }

    #[test]
    fn a_due_ui_frame_is_not_an_overlay_refresh() {
        assert!(is_overlay_refresh(true, false, false));
        assert!(!is_overlay_refresh(true, true, false));
        assert!(!is_overlay_refresh(true, false, true));
        assert!(!is_overlay_refresh(false, false, false));
    }

    #[test]
    fn earlier_picks_the_sooner_deadline() {
        let start = Instant::now();
        let sooner = start + Duration::from_millis(200);
        let later = start + OVERLAY_IDLE;
        assert_eq!(earlier(Some(later), Some(sooner)), Some(sooner));
        assert_eq!(earlier(None, Some(later)), Some(later));
        assert_eq!(earlier(None, None), None);
    }
}
