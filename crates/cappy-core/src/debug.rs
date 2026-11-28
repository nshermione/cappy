//! Frame stats a binding can subscribe to. The host fills them, and only while something is
//! subscribed. The signal is rooted in the runtime, so unmounting the component that first read
//! it does not dispose the slot.

use crate::Context;
use crate::reactive::{Reactive, Signal, runtime};

/// Counters from the latest presented frame.
///
/// Memory is `None` when this target cannot sample the process (wasm, and Linux). Draw counts
/// describe the paint that just finished, so a binding sees them on the following frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DebugStats {
    pub fps: u32,
    pub update_ms: f32,
    pub paint_ms: f32,
    /// Process footprint in bytes.
    pub private_bytes: Option<usize>,
    pub private_delta_kib: i64,
    pub live_nodes: usize,
    pub node_capacity: usize,
    pub primitives: usize,
    pub batches: usize,
    pub draw_calls: usize,
    pub damage_regions: usize,
    pub damage_percent: u32,
    pub live_signals: usize,
    pub bindings: usize,
    pub layout_nodes: usize,
    pub layout_retained: usize,
}

impl Context {
    /// The frame-stats signal. Reading it inside a binding subscribes that binding.
    ///
    /// The host samples process memory, arena counts and draw calls only while at least one
    /// binding is subscribed. A read outside a binding does not register.
    pub fn debug_stats(&self) -> Reactive<DebugStats> {
        let _ = self;
        Reactive::from_signal(signal())
    }
}

fn signal() -> Signal<DebugStats> {
    runtime().singleton_signal()
}

pub(crate) fn has_subscribers() -> bool {
    runtime()
        .existing_singleton::<Signal<DebugStats>>()
        .is_some_and(|signal| signal.subscriber_count() > 0)
}

/// Writes `stats` when a subscriber exists and the value changed.
///
/// Returns whether subscribers were notified. An equal snapshot does not wake the host, so an
/// idle refresh that already shows 0 fps does not schedule another frame.
pub(crate) fn publish(stats: DebugStats) -> bool {
    let Some(signal) = runtime().existing_singleton::<Signal<DebugStats>>() else {
        return false;
    };
    if signal.subscriber_count() == 0 || signal.get_untracked() == stats {
        return false;
    }
    signal.set(stats);
    true
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::reactive::{BindingId, BindingQueue, Observer, runtime, track};

    fn install() {
        crate::Application::new();
    }

    #[test]
    fn no_reader_means_nothing_to_publish() {
        install();
        assert!(!has_subscribers());
        assert!(!publish(DebugStats {
            fps: 1,
            ..DebugStats::default()
        }));
    }

    #[test]
    fn a_tracked_read_registers_and_an_equal_write_stays_quiet() {
        install();
        let queue = Rc::new(BindingQueue::new(runtime()));
        let stats = Context::current().debug_stats();
        track(Observer::new(&queue, BindingId::from_index(1)), || {
            let _ = stats.get();
        });
        assert!(has_subscribers());
        let next = DebugStats {
            fps: 4,
            ..DebugStats::default()
        };
        assert!(publish(next));
        assert_eq!(queue.take(), vec![BindingId::from_index(1)]);
        assert!(!publish(next));
        assert!(queue.is_empty());
    }

    #[test]
    fn dropping_the_binding_queue_drops_the_registration() {
        install();
        let queue = Rc::new(BindingQueue::new(runtime()));
        let stats = Context::current().debug_stats();
        track(Observer::new(&queue, BindingId::from_index(1)), || {
            let _ = stats.get();
        });
        drop(queue);
        assert!(!has_subscribers());
    }
}
