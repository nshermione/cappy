//! A floating readout of [`DebugStats`](cappy_core::DebugStats). Mounting it subscribes to the
//! stats signal, which is what makes the host start sampling.

use cappy_core::{
    Color, Component, Context, DebugStats, Element, Point, PointerEvents, div, text_dyn,
};
use cappy_dsl::style;

/// Top-left overlay of the current frame stats.
///
/// `element! { DebugPanel() }` mounts it. The panel does not take pointer hits. Removing it
/// drops the subscription, and the host stops sampling once nothing else is subscribed.
#[derive(Clone, Copy, Debug, Default)]
pub struct DebugPanel;

impl DebugPanel {
    /// A panel with no props. `element!` calls this.
    #[must_use]
    pub fn open() -> Self {
        Self
    }
}

impl Component for DebugPanel {
    fn render(&self) -> Element {
        let stats = Context::current().debug_stats();
        div()
            .overlay()
            .place(Point::new(8.0, 8.0))
            .style(style! {
                pointer_events: PointerEvents::None,
                padding: 8,
                background: Color::rgba(12, 14, 18, 210),
                text_color: Color::rgba(236, 236, 236, 255),
                font_size: 12,
                radius: 6,
            })
            .child(text_dyn(move || format_stats(&stats.get())))
    }
}

fn format_stats(stats: &DebugStats) -> String {
    let frame_ms = stats.update_ms + stats.paint_ms;
    let memory = match stats.private_bytes {
        Some(bytes) => {
            let mib = bytes as f64 / 1_048_576.0;
            format!("{mib:.1} MiB   {:+} KiB", stats.private_delta_kib)
        }
        None => "n/a".to_owned(),
    };
    format!(
        "{fps} fps   {frame_ms:.1} ms\n\
         upd {update:.1}   paint {paint:.1}\n\
         {memory}\n\
         nodes {live}/{cap}   prims {prims}\n\
         batch {batches}   draw {draws}   dmg {regions}  {percent}%\n\
         sig {signals}   bind {bindings}   lay {layout}/{retained}",
        fps = stats.fps,
        update = stats.update_ms,
        paint = stats.paint_ms,
        live = stats.live_nodes,
        cap = stats.node_capacity,
        prims = stats.primitives,
        batches = stats.batches,
        draws = stats.draw_calls,
        regions = stats.damage_regions,
        percent = stats.damage_percent,
        signals = stats.live_signals,
        bindings = stats.bindings,
        layout = stats.layout_nodes,
        retained = stats.layout_retained,
    )
}

#[cfg(test)]
mod tests {
    use cappy_core::{Application, DebugStats, FrameInput, Theme, Ui, div};

    use super::*;

    #[test]
    fn formats_the_six_lines() {
        let text = format_stats(&DebugStats {
            fps: 62,
            update_ms: 4.0,
            paint_ms: 4.5,
            private_bytes: Some(50 * 1024 * 1024),
            private_delta_kib: 120,
            live_nodes: 128,
            node_capacity: 160,
            primitives: 86,
            batches: 3,
            draw_calls: 18,
            damage_regions: 2,
            damage_percent: 4,
            live_signals: 12,
            bindings: 40,
            layout_nodes: 128,
            layout_retained: 140,
        });
        assert_eq!(
            text,
            "62 fps   8.5 ms\n\
             upd 4.0   paint 4.5\n\
             50.0 MiB   +120 KiB\n\
             nodes 128/160   prims 86\n\
             batch 3   draw 18   dmg 2  4%\n\
             sig 12   bind 40   lay 128/140"
        );
    }

    #[test]
    fn missing_process_memory_is_not_available() {
        let text = format_stats(&DebugStats::default());
        assert!(text.contains("\nn/a\n"));
    }

    #[test]
    fn mounting_the_panel_registers_until_it_is_dropped() {
        let _application = Application::new();
        let plain = Ui::new(div(), Theme::light());
        assert!(!plain.wants_debug_stats());
        drop(plain);

        let mut panel = Ui::new(DebugPanel::open(), Theme::light());
        panel.update(FrameInput::new(800, 600, 1.0));
        assert!(panel.wants_debug_stats());
        drop(panel);

        let again = Ui::new(div(), Theme::light());
        assert!(!again.wants_debug_stats());
    }
}
