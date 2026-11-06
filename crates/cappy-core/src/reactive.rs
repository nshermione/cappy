//! Fine-grained reactive signals.
//!
//! A [`Signal`] is a `Copy` handle to a value stored in a thread-local runtime. Reading a signal
//! inside a tracked scope subscribes that scope; writing the signal queues every subscriber so the
//! UI re-evaluates only the bindings that depend on it.
//!
//! [`reactive`] turns a primitive into a [`Reactive`] handle. `#[reactive]` on a struct keeps the
//! struct's name and tracks each field. A value that is already reactive is returned as itself.

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

// #region agent log
#[cfg(test)]
pub(crate) fn debug_signal_occupancy() -> (usize, usize, usize) {
    RUNTIME.with(|runtime| {
        let slots = runtime.slots.borrow();
        let occupied = slots.iter().filter(|slot| slot.is_some()).count();
        let free = runtime.free_slots.borrow().len();
        (occupied, slots.len(), free)
    })
}

#[cfg(test)]
pub(crate) fn debug_signal_alive<T: 'static>(signal: Signal<T>) -> bool {
    RUNTIME.with(|runtime| {
        runtime
            .slots
            .borrow()
            .get(signal.index as usize)
            .and_then(|slot| slot.as_ref())
            .is_some_and(|slot| slot.generation == signal.generation)
    })
}

#[cfg(test)]
pub(crate) fn debug_subscriber_count<T: 'static>(signal: Signal<T>) -> usize {
    RUNTIME.with(|runtime| {
        runtime
            .slots
            .borrow()
            .get(signal.index as usize)
            .and_then(|slot| slot.as_ref())
            .filter(|slot| slot.generation == signal.generation)
            .map(|slot| slot.subscribers.len())
            .unwrap_or(0)
    })
}
// #endregion

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

/// A `Copy` handle whose reads subscribe the current tracking scope.
///
/// Built by [`reactive`]. Formatting and `==` against the inner value read it. [`Reactive::set`]
/// and [`Reactive::update`] write it. The handle does not implement [`Deref`](std::ops::Deref):
/// the value lives in the thread-local runtime, not inside the handle.
pub struct Reactive<T: 'static> {
    inner: Signal<T>,
}

impl<T: 'static> Clone for Reactive<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Reactive<T> {}

impl<T: 'static> fmt::Debug for Reactive<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reactive")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<T: 'static> PartialEq for Reactive<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<T: 'static> Eq for Reactive<T> {}

impl<T: fmt::Display + 'static> fmt::Display for Reactive<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.with(|value| fmt::Display::fmt(value, formatter))
    }
}

impl<T: PartialEq + 'static> PartialEq<T> for Reactive<T> {
    fn eq(&self, other: &T) -> bool {
        self.inner.with(|value| value == other)
    }
}

impl<T: 'static> Reactive<T> {
    /// Reads the value and subscribes the current tracking scope.
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        self.inner.with(read)
    }

    /// Replaces the value and notifies subscribers.
    pub fn set(&self, value: T) {
        self.inner.set(value);
    }

    /// Mutates the value in place and notifies subscribers.
    pub fn update(&self, change: impl FnOnce(&mut T)) {
        self.inner.update(change);
    }
}

impl<T: Clone + 'static> Reactive<T> {
    /// Returns a clone of the value and subscribes the current tracking scope.
    pub fn get(&self) -> T {
        self.inner.get()
    }
}

/// A value [`reactive`] can turn into a tracked handle, or leave unchanged when it already is one.
pub trait IntoReactive {
    /// The tracked form of this value.
    type Output;

    /// Converts this value into its tracked form.
    fn into_reactive(self) -> Self::Output;
}

/// Tracks `value`.
///
/// A primitive or [`Option`] becomes one [`Reactive`] handle. A `#[reactive]` struct is already
/// tracked, so it is returned as itself. [`Reactive`] and [`Signal`] become a [`Reactive`] handle
/// without wrapping them again.
pub fn reactive<T: IntoReactive>(value: T) -> T::Output {
    value.into_reactive()
}

macro_rules! primitive_reactive {
    ($($ty:ty),* $(,)?) => {
        $(
            impl IntoReactive for $ty {
                type Output = Reactive<$ty>;

                fn into_reactive(self) -> Self::Output {
                    Reactive { inner: signal(self) }
                }
            }
        )*
    };
}

primitive_reactive!(
    bool, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl IntoReactive for String {
    type Output = Reactive<String>;

    fn into_reactive(self) -> Self::Output {
        Reactive {
            inner: signal(self),
        }
    }
}

impl<T: 'static> IntoReactive for Option<T> {
    type Output = Reactive<Option<T>>;

    fn into_reactive(self) -> Self::Output {
        Reactive {
            inner: signal(self),
        }
    }
}

impl<T: 'static> IntoReactive for Reactive<T> {
    type Output = Self;

    fn into_reactive(self) -> Self::Output {
        self
    }
}

impl<T: 'static> IntoReactive for Signal<T> {
    type Output = Reactive<T>;

    fn into_reactive(self) -> Self::Output {
        Reactive { inner: self }
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

    #[test]
    fn reactive_primitives_read_and_write() {
        let count = reactive(1_i64);
        assert_eq!(count.to_string(), "1");
        assert!(count == 1);
        count.set(2);
        assert_eq!(count.get(), 2);
        count.update(|value| *value += 1);
        assert_eq!(format!("{count}"), "3");

        let width = reactive(4_u32);
        assert_eq!(width.get(), 4);

        let selected = reactive(None::<usize>);
        selected.set(Some(1));
        assert_eq!(selected.get(), Some(1));
    }

    #[test]
    fn reactive_keeps_an_existing_handle() {
        let count = reactive(1_i64);
        assert_eq!(reactive(count), count);

        let inner = signal(String::from("hi"));
        let wrapped = reactive(inner);
        wrapped.set(String::from("yo"));
        assert_eq!(inner.get(), "yo");
        assert_eq!(reactive(wrapped), wrapped);
    }

    // #region agent log
    fn agent_log(hypothesis_id: &str, message: &str, data: String) {
        use std::io::Write;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let line = format!(
            "{{\"sessionId\":\"c37ca8\",\"runId\":\"probe\",\"hypothesisId\":\"{hypothesis_id}\",\"location\":\"cappy-core/src/reactive.rs\",\"message\":{message_json},\"data\":{data},\"timestamp\":{timestamp}}}\n",
            message_json = format!("\"{}\"", message.replace('"', "'")),
        );
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(r"d:\projects\rs-gui\.cursor\debug-c37ca8.log")
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    #[test]
    fn memory_probe_dynamic_slots() {
        let (occupied_before, _, _) = debug_signal_occupancy();
        {
            let _dropped = signal(String::from("temporary"));
        }
        let (occupied_after_drop, _, _) = debug_signal_occupancy();
        let kept = signal(1);
        kept.dispose();
        let (occupied_after_dispose, _, free_after_dispose) = debug_signal_occupancy();
        agent_log(
            "A",
            "signal drop vs dispose",
            format!(
                "{{\"occupiedBefore\":{occupied_before},\"occupiedAfterDrop\":{occupied_after_drop},\"dropRetained\":{},\"occupiedAfterDispose\":{occupied_after_dispose},\"freeAfterDispose\":{free_after_dispose}}}",
                occupied_after_drop == occupied_before + 1
            ),
        );

        let (listeners_before, handlers_before) = crate::listener::debug_listener_occupancy();
        {
            let listener = crate::Listener::<String>::empty();
            listener.set(|value| {
                let _kept = value.clone();
            });
        }
        let (listeners_after, handlers_after) = crate::listener::debug_listener_occupancy();
        agent_log(
            "B",
            "listener drop keeps slot and handler",
            format!(
                "{{\"slotsBefore\":{listeners_before},\"slotsAfter\":{listeners_after},\"handlersBefore\":{handlers_before},\"handlersAfter\":{handlers_after}}}"
            ),
        );

        let (before_prop, _, _) = debug_signal_occupancy();
        let mut slot = reactive(String::from("default-prop"));
        let orphan = slot.inner;
        let parent = reactive(String::from("parent"));
        crate::PropInput::<String, crate::ByHandle>::apply(parent, &mut slot);
        let orphan_alive = debug_signal_alive(orphan);
        let (after_prop, _, _) = debug_signal_occupancy();
        agent_log(
            "C",
            "by-handle prop replace",
            format!(
                "{{\"occupiedBefore\":{before_prop},\"occupiedAfter\":{after_prop},\"orphanAlive\":{orphan_alive},\"slotNowAliasesParent\":{}}}",
                slot.inner == parent.inner
            ),
        );

        let binding_queue = queue();
        let flag = signal(true);
        let left = signal(1);
        let right = signal(2);
        let eval = || {
            track(Observer::new(&binding_queue, BindingId(1)), || {
                if flag.get() { left.get() } else { right.get() }
            });
        };
        eval();
        let left_after_first = debug_subscriber_count(left);
        let right_after_first = debug_subscriber_count(right);
        flag.set(false);
        eval();
        let left_after_second = debug_subscriber_count(left);
        let right_after_second = debug_subscriber_count(right);
        agent_log(
            "D",
            "binding does not unsubscribe",
            format!(
                "{{\"leftAfterFirst\":{left_after_first},\"rightAfterFirst\":{right_after_first},\"leftAfterSecond\":{left_after_second},\"rightAfterSecond\":{right_after_second}}}"
            ),
        );

        let watched = signal(0);
        {
            let ephemeral = queue();
            track(Observer::new(&ephemeral, BindingId(7)), || watched.get());
        }
        let subscribers_after_queue_drop = debug_subscriber_count(watched);
        watched.set(1);
        let subscribers_after_write = debug_subscriber_count(watched);
        agent_log(
            "E",
            "dead observers linger until write",
            format!(
                "{{\"afterQueueDrop\":{subscribers_after_queue_drop},\"afterWrite\":{subscribers_after_write}}}"
            ),
        );

        let (before_ui, _, _) = debug_signal_occupancy();
        let theme_signal;
        {
            let ui = crate::Ui::new(crate::div(), crate::Theme::light());
            theme_signal = ui.theme_signal();
        }
        let (after_ui_drop, _, _) = debug_signal_occupancy();
        agent_log(
            "G",
            "Ui::new theme signal survives Ui drop",
            format!(
                "{{\"occupiedBefore\":{before_ui},\"occupiedAfter\":{after_ui_drop},\"themeAliveAfterDrop\":{}}}",
                debug_signal_alive(theme_signal)
            ),
        );

        let cycled = queue();
        let captured = Rc::clone(&cycled);
        cycled.set_waker(Some(Rc::new(move || {
            let _pin = &captured;
        })));
        let cycled_weak = Rc::downgrade(&cycled);
        drop(cycled);
        let plain = queue();
        plain.set_waker(Some(Rc::new(|| {})));
        let plain_weak = Rc::downgrade(&plain);
        drop(plain);
        agent_log(
            "F",
            "waker capture cycle",
            format!(
                "{{\"capturingWakerStrongCount\":{},\"plainWakerStrongCount\":{}}}",
                cycled_weak.strong_count(),
                plain_weak.strong_count()
            ),
        );
    }
    // #endregion

    #[test]
    fn display_read_tracks_in_a_binding() {
        use crate::{FrameInput, Theme, Ui, div, text_dyn};

        let count = reactive(1_i64);
        let mut ui = Ui::new(
            div().child(text_dyn(move || format!("{count}")).key("label")),
            Theme::light(),
        );
        let viewport = FrameInput::new(320, 240, 1.0);
        ui.update(viewport);
        let label = ui.find_by_key("label").unwrap();
        assert_eq!(ui.node_text(label), Some("1"));
        count.set(2);
        ui.update(viewport);
        assert_eq!(ui.node_text(label), Some("2"));
    }
}
