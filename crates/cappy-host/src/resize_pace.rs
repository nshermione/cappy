//! How often a live resize is allowed to present.
//!
//! Each presented size recreates the window buffer. While the pointer is still dragging, presents
//! are at least one resize interval apart and the window size follows the pointer. Once the size
//! stops changing, the next redraw presents that size. The CPU backend uses a longer interval
//! because a size change repaints the whole window on the UI thread.

use std::time::{Duration, Instant};

use crate::RendererKind;

/// Present interval while the size is still changing on the GPU backend.
pub(crate) const RESIZE_INTERVAL: Duration = Duration::from_millis(16);
/// Present interval while the size is still changing on the CPU backend.
pub(crate) const CPU_RESIZE_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn resize_interval(kind: RendererKind) -> Duration {
    match kind {
        RendererKind::Cpu => CPU_RESIZE_INTERVAL,
        RendererKind::Gpu => RESIZE_INTERVAL,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PresentedFrame {
    pub size: (u32, u32),
    pub scale_factor: f32,
    pub at: Instant,
}

pub(crate) enum ResizeFrame {
    Paint,
    /// Wait this long, then redraw. The wait is the sooner of the present interval and the idle
    /// timeout, so a released drag presents without waiting out the whole interval.
    Defer {
        after: Duration,
    },
}

pub(crate) fn choose_resize_frame(
    presented: Option<PresentedFrame>,
    size: (u32, u32),
    scale: f32,
    last_resize: Option<Instant>,
    now: Instant,
    interval: Duration,
) -> ResizeFrame {
    let Some(presented) = presented else {
        return ResizeFrame::Paint;
    };
    if presented.size == size || presented.scale_factor != scale {
        return ResizeFrame::Paint;
    }
    let resizing = last_resize.is_some_and(|at| now.saturating_duration_since(at) < interval);
    if !resizing {
        return ResizeFrame::Paint;
    }
    let since_present = now.saturating_duration_since(presented.at);
    if since_present >= interval {
        return ResizeFrame::Paint;
    }
    let until_interval = interval - since_present;
    let until_idle = last_resize
        .map(|at| interval.saturating_sub(now.saturating_duration_since(at)))
        .unwrap_or(until_interval);
    let after = until_interval.min(until_idle);
    if after.is_zero() {
        return ResizeFrame::Paint;
    }
    ResizeFrame::Defer { after }
}

#[cfg(test)]
mod tests {
    use super::{
        CPU_RESIZE_INTERVAL, PresentedFrame, RESIZE_INTERVAL, ResizeFrame, choose_resize_frame,
    };
    use std::time::{Duration, Instant};

    fn presented(at: Instant) -> PresentedFrame {
        PresentedFrame {
            size: (800, 600),
            scale_factor: 1.0,
            at,
        }
    }

    #[test]
    fn the_first_frame_paints() {
        let now = Instant::now();
        assert!(matches!(
            choose_resize_frame(None, (800, 600), 1.0, Some(now), now, RESIZE_INTERVAL),
            ResizeFrame::Paint
        ));
    }

    #[test]
    fn an_unchanged_size_paints() {
        let now = Instant::now();
        assert!(matches!(
            choose_resize_frame(
                Some(presented(now)),
                (800, 600),
                1.0,
                Some(now),
                now,
                RESIZE_INTERVAL
            ),
            ResizeFrame::Paint
        ));
    }

    #[test]
    fn a_scale_change_paints() {
        let now = Instant::now();
        assert!(matches!(
            choose_resize_frame(
                Some(presented(now)),
                (1600, 1200),
                2.0,
                Some(now),
                now,
                RESIZE_INTERVAL
            ),
            ResizeFrame::Paint
        ));
    }

    #[test]
    fn a_drag_defers_until_the_interval() {
        let now = Instant::now();
        let presented = presented(now);
        let soon = now + Duration::from_millis(10);
        match choose_resize_frame(
            Some(presented),
            (900, 700),
            1.0,
            Some(soon),
            soon,
            RESIZE_INTERVAL,
        ) {
            ResizeFrame::Defer { after } => {
                assert!(after < RESIZE_INTERVAL);
            }
            ResizeFrame::Paint => panic!("a fresh drag should wait"),
        }
    }

    #[test]
    fn a_drag_paints_once_the_interval_has_elapsed() {
        let now = Instant::now();
        let presented = presented(now);
        let later = now + RESIZE_INTERVAL;
        assert!(matches!(
            choose_resize_frame(
                Some(presented),
                (900, 700),
                1.0,
                Some(later),
                later,
                RESIZE_INTERVAL
            ),
            ResizeFrame::Paint
        ));
    }

    #[test]
    fn a_settled_size_paints() {
        let now = Instant::now();
        let presented = presented(now);
        let later = now + RESIZE_INTERVAL;
        assert!(matches!(
            choose_resize_frame(
                Some(presented),
                (900, 700),
                1.0,
                Some(now),
                later,
                RESIZE_INTERVAL
            ),
            ResizeFrame::Paint
        ));
    }

    #[test]
    fn a_cpu_drag_uses_the_longer_interval() {
        let now = Instant::now();
        let presented = presented(now);
        let mid = now + Duration::from_millis(50);
        assert!(matches!(
            choose_resize_frame(
                Some(presented),
                (900, 700),
                1.0,
                Some(mid),
                mid,
                CPU_RESIZE_INTERVAL
            ),
            ResizeFrame::Defer { .. }
        ));
        let later = now + CPU_RESIZE_INTERVAL;
        assert!(matches!(
            choose_resize_frame(
                Some(presented),
                (900, 700),
                1.0,
                Some(later),
                later,
                CPU_RESIZE_INTERVAL
            ),
            ResizeFrame::Paint
        ));
    }
}
