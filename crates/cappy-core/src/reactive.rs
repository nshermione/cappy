//! Fine-grained reactive signals.
//!
//! A [`Signal`] is a `Copy` handle to a value stored in a thread-local runtime. Reading a signal
//! inside a tracked scope subscribes that scope; writing the signal queues every subscriber so the
//! UI re-evaluates only the bindings that depend on it.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

/// Identifies one reactive binding inside a [`BindingQueue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct BindingId(pub(crate) u32);

/// Bindings whose dependencies changed and must be re-evaluated.
#[derive(Default)]
pub(crate) struct BindingQueue {
    pending: RefCell<Vec<BindingId>>,
    waker: RefCell<Option<Rc<dyn Fn()>>>,
}

impl BindingQueue {
    pub(crate) fn set_waker(&self, waker: Option<Rc<dyn Fn()>>) {
        *self.waker.borrow_mut() = waker;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }

    pub(crate) fn take(&self) -> Vec<BindingId> {
        let mut pending = std::mem::take(&mut *self.pending.borrow_mut());
        pending.sort_unstable();
        pending.dedup();
        pending
    }

    fn push(&self, binding: BindingId) {
        let was_empty = {
            let mut pending = self.pending.borrow_mut();
            let was_empty = pending.is_empty();
            pending.push(binding);
            was_empty
        };
        if was_empty {
            let waker = self.waker.borrow().clone();
            if let Some(waker) = waker {
                waker();
            }
        }
    }
}

/// A subscriber: one binding of one UI.
#[derive(Clone)]
pub(crate) struct Observer {
    queue: Weak<BindingQueue>,
    binding: BindingId,
}

impl Observer {
    pub(crate) fn new(queue: &Rc<BindingQueue>, binding: BindingId) -> Self {
        Self {
            queue: Rc::downgrade(queue),
            binding,
        }
    }

    fn same_as(&self, other: &Observer) -> bool {
        self.binding == other.binding && Weak::ptr_eq(&self.queue, &other.queue)
    }

    fn is_alive(&self) -> bool {
        self.queue.strong_count() > 0
    }

    fn notify(&self) {
        if let Some(queue) = self.queue.upgrade() {
            queue.push(self.binding);
        }
    }
}

type SharedValue = Rc<RefCell<Box<dyn Any>>>;

struct SignalSlot {
    generation: u32,
    value: SharedValue,
    subscribers: Vec<Observer>,
}

#[derive(Default)]
struct Runtime {
    slots: RefCell<Vec<Option<SignalSlot>>>,
    free_slots: RefCell<Vec<u32>>,
    next_generation: RefCell<u32>,
    current_observer: RefCell<Option<Observer>>,
}

thread_local! {
    static RUNTIME: Runtime = Runtime::default();
}

const DEAD_SIGNAL: &str =
    "signal was disposed, or used on a thread other than the one that created it";

impl Runtime {
    fn create(&self, value: Box<dyn Any>) -> (u32, u32) {
        let generation = {
            let mut next = self.next_generation.borrow_mut();
            *next = next.wrapping_add(1);
            *next
        };
        let slot = SignalSlot {
            generation,
            value: Rc::new(RefCell::new(value)),
            subscribers: Vec::new(),
        };
        let mut slots = self.slots.borrow_mut();
        let index = match self.free_slots.borrow_mut().pop() {
            Some(index) => {
                slots[index as usize] = Some(slot);
                index
            }
            None => {
                slots.push(Some(slot));
                u32::try_from(slots.len() - 1).expect("too many live signals")
            }
        };
        (index, generation)
    }

    fn with_slot<R>(&self, index: u32, generation: u32, f: impl FnOnce(&mut SignalSlot) -> R) -> R {
        let mut slots = self.slots.borrow_mut();
        let slot = slots
            .get_mut(index as usize)
            .and_then(Option::as_mut)
            .filter(|slot| slot.generation == generation)
            .expect(DEAD_SIGNAL);
        f(slot)
    }

    fn read(&self, index: u32, generation: u32, track: bool) -> SharedValue {
        let observer = if track {
            self.current_observer.borrow().clone()
        } else {
            None
        };
        self.with_slot(index, generation, |slot| {
            if let Some(observer) = observer
                && !slot
                    .subscribers
                    .iter()
                    .any(|existing| existing.same_as(&observer))
            {
                slot.subscribers.push(observer);
            }
            slot.value.clone()
        })
    }

    fn subscribers(&self, index: u32, generation: u32) -> Vec<Observer> {
        self.with_slot(index, generation, |slot| {
            slot.subscribers.retain(Observer::is_alive);
            slot.subscribers.clone()
        })
    }

    fn dispose(&self, index: u32, generation: u32) {
        let mut slots = self.slots.borrow_mut();
        if let Some(entry) = slots.get_mut(index as usize)
            && entry
                .as_ref()
                .is_some_and(|slot| slot.generation == generation)
        {
            *entry = None;
            self.free_slots.borrow_mut().push(index);
        }
    }
}

/// Runs `scope` with `observer` as the current subscriber, restoring the previous one afterwards.
pub(crate) fn track<R>(observer: Observer, scope: impl FnOnce() -> R) -> R {
    let previous = RUNTIME.with(|runtime| runtime.current_observer.replace(Some(observer)));
    let result = scope();
    RUNTIME.with(|runtime| *runtime.current_observer.borrow_mut() = previous);
    result
}

/// A reactive value. Cheap to copy; all copies refer to the same value.
///
/// Signals live on the thread that created them and stay alive until [`Signal::dispose`].
pub struct Signal<T: 'static> {
    index: u32,
    generation: u32,
    _value: PhantomData<fn() -> T>,
    _not_send: PhantomData<*const ()>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Signal<T> {}

impl<T: 'static> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T: 'static> Eq for Signal<T> {}

impl<T: 'static> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

/// Creates a signal. Shorthand for [`Signal::new`].
pub fn signal<T: 'static>(value: T) -> Signal<T> {
    Signal::new(value)
}

impl<T: 'static> Signal<T> {
    pub fn new(value: T) -> Self {
        let (index, generation) = RUNTIME.with(|runtime| runtime.create(Box::new(value)));
        Self {
            index,
            generation,
            _value: PhantomData,
            _not_send: PhantomData,
        }
    }

    /// Reads the value and subscribes the current tracking scope.
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        self.read_value(true, read)
    }

    /// Reads the value without subscribing.
    pub fn with_untracked<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        self.read_value(false, read)
    }

    /// Replaces the value and notifies subscribers.
    pub fn set(&self, value: T) {
        self.update(|current| *current = value);
    }

    /// Mutates the value in place and notifies subscribers.
    pub fn update(&self, change: impl FnOnce(&mut T)) {
        let value = RUNTIME.with(|runtime| runtime.read(self.index, self.generation, false));
        {
            let mut value = value.borrow_mut();
            change(value.downcast_mut::<T>().expect(DEAD_SIGNAL));
        }
        let subscribers = RUNTIME.with(|runtime| runtime.subscribers(self.index, self.generation));
        for subscriber in subscribers {
            subscriber.notify();
        }
    }

    /// Frees the value. Using any copy of this signal afterwards panics.
    pub fn dispose(self) {
        RUNTIME.with(|runtime| runtime.dispose(self.index, self.generation));
    }

    fn read_value<R>(&self, track: bool, read: impl FnOnce(&T) -> R) -> R {
        let value = RUNTIME.with(|runtime| runtime.read(self.index, self.generation, track));
        let value = value.borrow();
        read(value.downcast_ref::<T>().expect(DEAD_SIGNAL))
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// Returns a clone of the value and subscribes the current tracking scope.
    pub fn get(&self) -> T {
        self.with(T::clone)
    }

    pub fn get_untracked(&self) -> T {
        self.with_untracked(T::clone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn queue() -> Rc<BindingQueue> {
        Rc::new(BindingQueue::default())
    }

    #[test]
    fn get_set_update_round_trip() {
        let count = signal(1);
        count.set(5);
        count.update(|value| *value += 1);
        assert_eq!(count.get(), 6);
    }

    #[test]
    fn copies_share_the_same_value() {
        let name = signal(String::from("a"));
        let copy = name;
        copy.update(|value| value.push('b'));
        assert_eq!(name.get(), "ab");
    }

    #[test]
    fn tracked_reads_subscribe_and_writes_notify() {
        let queue = queue();
        let count = signal(0);
        track(Observer::new(&queue, BindingId(3)), || count.get());
        assert!(queue.is_empty());
        count.set(1);
        assert_eq!(queue.take(), vec![BindingId(3)]);
    }

    #[test]
    fn untracked_reads_do_not_subscribe() {
        let queue = queue();
        let count = signal(0);
        track(Observer::new(&queue, BindingId(1)), || {
            count.get_untracked()
        });
        count.set(1);
        assert!(queue.is_empty());
    }

    #[test]
    fn repeated_tracking_subscribes_once() {
        let queue = queue();
        let count = signal(0);
        for _ in 0..3 {
            track(Observer::new(&queue, BindingId(1)), || count.get());
        }
        count.set(1);
        count.set(2);
        assert_eq!(queue.take(), vec![BindingId(1)]);
    }

    #[test]
    fn nested_scopes_restore_the_outer_observer() {
        let queue = queue();
        let outer = signal(0);
        let inner = signal(0);
        track(Observer::new(&queue, BindingId(1)), || {
            track(Observer::new(&queue, BindingId(2)), || inner.get());
            outer.get()
        });
        outer.set(1);
        assert_eq!(queue.take(), vec![BindingId(1)]);
        inner.set(1);
        assert_eq!(queue.take(), vec![BindingId(2)]);
    }

    #[test]
    fn dropped_queue_stops_receiving() {
        let count = signal(0);
        {
            let queue = queue();
            track(Observer::new(&queue, BindingId(1)), || count.get());
        }
        count.set(1);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn waker_fires_once_per_batch() {
        let queue = queue();
        let wakes = Rc::new(Cell::new(0));
        let counter = wakes.clone();
        queue.set_waker(Some(Rc::new(move || counter.set(counter.get() + 1))));
        let count = signal(0);
        track(Observer::new(&queue, BindingId(1)), || count.get());
        count.set(1);
        count.set(2);
        assert_eq!(wakes.get(), 1);
        queue.take();
        count.set(3);
        assert_eq!(wakes.get(), 2);
    }

    #[test]
    #[should_panic(expected = "signal was disposed")]
    fn using_a_disposed_signal_panics() {
        let count = signal(0);
        count.dispose();
        count.get();
    }

    #[test]
    fn disposed_slots_are_reused_without_aliasing() {
        let first = signal(1);
        first.dispose();
        let second = signal(2);
        assert_ne!(first, second);
        assert_eq!(second.get(), 2);
    }
}
