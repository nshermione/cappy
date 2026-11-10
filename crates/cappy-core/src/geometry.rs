//! Layout rect and scroll offset of keyed nodes, readable from a component binding.
//!
//! Each [`crate::Ui`] owns one [`GeometryBoard`]. Layout copies the rect and scroll of every keyed
//! node into that board. A binding that reads a key subscribes, and the next change queues it
//! again. A write is applied on the next update of the UI that was current, or of the first UI
//! when the call is outside an update, and that write wakes the host.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use cappy_layout::{Point, Rect};

use crate::reactive::{BindingId, BindingQueue, Observer, Runtime};
use crate::{Context, NodeId};

/// One key a binding read the last time it ran.
struct ReadKey {
    key: String,
    rect: bool,
    scroll: bool,
}

struct Slot {
    /// Lowest arena index that currently owns this key. Matches [`crate::Ui::find_by_key`].
    id: Option<NodeId>,
    rect: Option<Rect>,
    scroll: Option<Point>,
    rect_watchers: Vec<Observer>,
    scroll_watchers: Vec<Observer>,
}

/// Rects and scroll offsets one UI publishes for its keyed nodes.
#[derive(Default)]
pub(crate) struct GeometryBoard {
    slots: RefCell<HashMap<String, Slot>>,
    /// Keys each binding subscribed to, so the next run can drop the ones it no longer reads.
    reads: RefCell<HashMap<BindingId, Vec<ReadKey>>>,
    writes: RefCell<Vec<(String, Point)>>,
    /// Fires when `writes` goes from empty to occupied. The UI installs the same waker as its
    /// binding queue.
    waker: RefCell<Option<Rc<dyn Fn()>>>,
    /// A publish notified at least one binding. The UI flushes those bindings before paint.
    notified: Cell<bool>,
}

impl GeometryBoard {
    pub(crate) fn set_waker(&self, waker: Option<Rc<dyn Fn()>>) {
        *self.waker.borrow_mut() = waker;
    }

    pub(crate) fn has_writes(&self) -> bool {
        !self.writes.borrow().is_empty()
    }

    pub(crate) fn take_writes(&self) -> Vec<(String, Point)> {
        std::mem::take(&mut *self.writes.borrow_mut())
    }

    pub(crate) fn take_notified(&self) -> bool {
        self.notified.replace(false)
    }

    /// Drops geometry subscriptions this binding took on its previous run.
    pub(crate) fn detach(&self, queue: &BindingQueue, binding: BindingId) {
        let Some(reads) = self.reads.borrow_mut().remove(&binding) else {
            return;
        };
        let queue_ptr = queue as *const BindingQueue;
        let mut slots = self.slots.borrow_mut();
        for read in reads {
            let Some(slot) = slots.get_mut(&read.key) else {
                continue;
            };
            if read.rect {
                slot.rect_watchers
                    .retain(|watcher| !watcher.belongs_to(binding, queue_ptr));
            }
            if read.scroll {
                slot.scroll_watchers
                    .retain(|watcher| !watcher.belongs_to(binding, queue_ptr));
            }
        }
    }

    pub(crate) fn read_rect(&self, runtime: &Runtime, key: &str) -> Option<Rect> {
        self.observe(runtime, key, true);
        self.slots.borrow().get(key).and_then(|slot| slot.rect)
    }

    pub(crate) fn read_scroll(&self, runtime: &Runtime, key: &str) -> Option<Point> {
        self.observe(runtime, key, false);
        self.slots.borrow().get(key).and_then(|slot| slot.scroll)
    }

    /// Queues `offset` for the node named `key`. The first write in a batch wakes the host.
    pub(crate) fn write_scroll(&self, key: &str, offset: Point) {
        let wake = {
            let mut writes = self.writes.borrow_mut();
            let wake = writes.is_empty();
            writes.push((key.to_owned(), offset));
            wake
        };
        if wake {
            let waker = self.waker.borrow().clone();
            if let Some(waker) = waker {
                waker();
            }
        }
    }

    /// Stores the layout result for `key`. The node with the lowest arena index keeps the key.
    pub(crate) fn publish(&self, key: &str, id: NodeId, rect: Rect, scroll: Point) {
        let (rect_watchers, scroll_watchers) = {
            let mut slots = self.slots.borrow_mut();
            let slot = if let Some(slot) = slots.get_mut(key) {
                slot
            } else {
                slots.insert(key.to_owned(), occupied(id, rect, scroll));
                return;
            };
            if slot
                .id
                .is_some_and(|owner| owner != id && owner.index() < id.index())
            {
                return;
            }
            let rect_changed = slot.rect != Some(rect);
            let scroll_changed = slot.scroll != Some(scroll);
            slot.id = Some(id);
            slot.rect = Some(rect);
            slot.scroll = Some(scroll);
            (
                if rect_changed {
                    slot.rect_watchers.clone()
                } else {
                    Vec::new()
                },
                if scroll_changed {
                    slot.scroll_watchers.clone()
                } else {
                    Vec::new()
                },
            )
        };
        self.notify(rect_watchers);
        self.notify(scroll_watchers);
    }

    /// The node that owned `key` is gone. Readers see `None` until another node takes the key.
    pub(crate) fn forget(&self, key: &str, id: NodeId) {
        let (rect_watchers, scroll_watchers) = {
            let mut slots = self.slots.borrow_mut();
            let Some(slot) = slots.get_mut(key) else {
                return;
            };
            if slot.id != Some(id) {
                return;
            }
            let changed = slot.rect.take().is_some() || slot.scroll.take().is_some();
            slot.id = None;
            if !changed {
                if slot.rect_watchers.is_empty() && slot.scroll_watchers.is_empty() {
                    slots.remove(key);
                }
                return;
            }
            if slot.rect_watchers.is_empty() && slot.scroll_watchers.is_empty() {
                slots.remove(key);
                return;
            }
            (slot.rect_watchers.clone(), slot.scroll_watchers.clone())
        };
        self.notify(rect_watchers);
        self.notify(scroll_watchers);
    }

    fn observe(&self, runtime: &Runtime, key: &str, rect: bool) {
        let Some(observer) = runtime.current_observer.borrow().clone() else {
            return;
        };
        let binding = observer.binding_id();
        {
            let mut reads = self.reads.borrow_mut();
            let reads = reads.entry(binding).or_default();
            if let Some(existing) = reads.iter_mut().find(|read| read.key == key) {
                if rect {
                    existing.rect = true;
                } else {
                    existing.scroll = true;
                }
            } else {
                reads.push(ReadKey {
                    key: key.to_owned(),
                    rect,
                    scroll: !rect,
                });
            }
        }
        let mut slots = self.slots.borrow_mut();
        let slot = if let Some(slot) = slots.get_mut(key) {
            slot
        } else {
            slots.insert(key.to_owned(), Slot::empty());
            slots.get_mut(key).expect("key was just inserted")
        };
        let watchers = if rect {
            &mut slot.rect_watchers
        } else {
            &mut slot.scroll_watchers
        };
        if watchers.iter().any(|existing| existing.same_as(&observer)) {
            return;
        }
        watchers.push(observer);
    }

    fn notify(&self, watchers: Vec<Observer>) {
        if watchers.is_empty() {
            return;
        }
        self.notified.set(true);
        for watcher in watchers {
            watcher.notify();
        }
    }
}

impl Slot {
    fn empty() -> Self {
        Self {
            id: None,
            rect: None,
            scroll: None,
            rect_watchers: Vec::new(),
            scroll_watchers: Vec::new(),
        }
    }
}

fn occupied(id: NodeId, rect: Rect, scroll: Point) -> Slot {
    Slot {
        id: Some(id),
        rect: Some(rect),
        scroll: Some(scroll),
        rect_watchers: Vec::new(),
        scroll_watchers: Vec::new(),
    }
}

impl Context {
    /// Layout rect of the node named `key`, from the last layout of the UI currently updating.
    ///
    /// `None` when that UI has no such key, or layout has not placed it yet. A read inside a
    /// binding runs that binding again when the rect changes. Outside an update or event, this
    /// returns `None`.
    pub fn node_rect(&self, key: &str) -> Option<Rect> {
        let board = self.runtime.current_board()?;
        board.read_rect(&self.runtime, key)
    }

    /// Scroll offset of the node named `key`, in logical pixels.
    ///
    /// Positive x and y move the content left and up. `None` when the key is missing. A read
    /// inside a binding runs that binding again when the offset changes, including while it eases.
    /// A rect change alone does not. Outside an update or event, this returns `None`.
    pub fn scroll_offset(&self, key: &str) -> Option<Point> {
        let board = self.runtime.current_board()?;
        board.read_scroll(&self.runtime, key)
    }

    /// Sets the scroll offset of the node named `key` and skips easing.
    ///
    /// The offset is clamped to what that node can scroll on the next update. An unknown key does
    /// nothing. The write wakes the host: the UI whose update or event is running, or the first UI
    /// on this context when the call is outside both. A call before any UI exists is ignored.
    pub fn set_scroll_offset(&self, key: &str, offset: Point) {
        let Some(board) = self.runtime.board_for_write() else {
            return;
        };
        board.write_scroll(key, offset);
    }
}
