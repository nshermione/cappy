//! One-shot timers on keyed nodes.
//!
//! [`Context::timeout`] queues a command on the UI's geometry board. The next update arms it.
//! [`Ui::frame_deadline`] wakes the host at the deadline. The callback runs once, before bindings,
//! and disposing the node drops it.

use std::rc::Rc;

use crate::geometry::TimerCommand;
use crate::reactive::enter_runtime;
use crate::time::{Duration, Instant};
use crate::{Context, NodeId};

/// A callback waiting on one node.
pub(super) struct ArmedTimer {
    id: NodeId,
    deadline: Instant,
    callback: Box<dyn FnOnce()>,
}

impl super::Ui {
    /// Moves queued arm and clear commands onto nodes. The clock starts at `now`, not at the call.
    pub(super) fn drain_timer_commands(&mut self, now: Instant) {
        for command in self.board.take_timer_commands() {
            match command {
                TimerCommand::Arm {
                    key,
                    delay,
                    callback,
                } => {
                    let Some(id) = self.find_by_key(&key) else {
                        continue;
                    };
                    self.arm_timer(id, now + delay, callback);
                }
                TimerCommand::Clear { key } => {
                    let Some(id) = self.find_by_key(&key) else {
                        continue;
                    };
                    self.cancel_timer(id);
                }
            }
        }
    }

    /// Runs every armed callback whose deadline is at or before `now`. Each callback runs once.
    pub(super) fn fire_due_timers(&mut self, now: Instant) {
        let mut pending = Vec::new();
        let mut waiting = Vec::new();
        for timer in self.timers.drain(..) {
            if timer.deadline <= now {
                pending.push(timer);
            } else {
                waiting.push(timer);
            }
        }
        self.timers = waiting;
        let runtime = Rc::clone(&self.queue.runtime);
        for timer in pending {
            if !self.is_current(timer.id) {
                continue;
            }
            enter_runtime(&runtime, timer.callback);
        }
    }

    /// Drops the callback armed on `id`, if there is one.
    pub(super) fn cancel_timer(&mut self, id: NodeId) {
        self.timers.retain(|timer| timer.id != id);
    }

    /// Soonest armed deadline. `None` when nothing is waiting.
    pub(super) fn timer_deadline(&self) -> Option<Instant> {
        self.timers.iter().map(|timer| timer.deadline).min()
    }

    fn arm_timer(&mut self, id: NodeId, deadline: Instant, callback: Box<dyn FnOnce()>) {
        if let Some(timer) = self.timers.iter_mut().find(|timer| timer.id == id) {
            timer.deadline = deadline;
            timer.callback = callback;
            return;
        }
        self.timers.push(ArmedTimer {
            id,
            deadline,
            callback,
        });
    }
}

impl Context {
    /// Runs `callback` once, on a later update, after `delay`.
    ///
    /// The timer belongs to the node named `key`, the same node [`Ui::find_by_key`](crate::Ui::find_by_key)
    /// returns. A second call for that key replaces the pending callback, and the earlier one does
    /// not run. [`Context::clear_timeout`] drops it without running it. Disposing the node does
    /// the same. Hiding it with `show(false)` or `display: none` does not.
    ///
    /// The deadline is the `now` of the update that arms the timer, plus `delay`. The callback
    /// does not run inside this call. A delay of zero runs in that update, before bindings, so a
    /// signal it writes is painted then. A timer scheduled from a callback waits for the next
    /// update. The callback itself does not mark pixels dirty.
    ///
    /// The host sleeps until the deadline. While the timer is armed, [`Ui::needs_update`](crate::Ui::needs_update)
    /// stays false. An unknown key never runs the callback. When a UI exists, that key still wakes
    /// the host once so the command can be dropped. A call before any UI exists is ignored.
    ///
    /// From an update or event, the timer is armed on that UI. Otherwise it is armed on the first
    /// UI of this context. Two windows do not share timers.
    pub fn timeout(&self, key: &str, delay: Duration, callback: impl FnOnce() + 'static) {
        let Some(board) = self.runtime.board_for_write() else {
            return;
        };
        board.push_timer_command(TimerCommand::Arm {
            key: key.to_owned(),
            delay,
            callback: Box::new(callback),
        });
    }

    /// Drops the pending [`Context::timeout`] on the node named `key` without running it.
    ///
    /// An unknown key wakes the host once and then does nothing. A call before any UI exists is
    /// ignored. Which UI receives the clear matches [`Context::timeout`].
    pub fn clear_timeout(&self, key: &str) {
        let Some(board) = self.runtime.board_for_write() else {
            return;
        };
        board.push_timer_command(TimerCommand::Clear {
            key: key.to_owned(),
        });
    }
}
