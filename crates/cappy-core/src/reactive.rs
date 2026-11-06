//! Fine-grained reactive signals.
//!
//! A [`Signal`] is a `Copy` handle to a value stored in a thread-local runtime. Reading a signal
//! inside a tracked scope subscribes that scope; writing the signal queues every subscriber so the
//! UI re-evaluates only the bindings that depend on it.
//!
//! [`reactive`] turns a primitive into a [`Reactive`] handle. `#[reactive]` on a struct keeps the
//! struct's name and tracks each field. A value that is already reactive is returned as itself.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

/// Identifies one reactive binding inside a [`BindingQueue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct BindingId(pub(crate) u32);

/// One signal a binding read the last time it ran.
#[derive(Clone, Copy)]
struct SignalKey {
    index: u32,
    generation: u32,
}

/// Bindings whose dependencies changed and must be re-evaluated.
///
/// `sources` records the signals each binding subscribed to. The next run detaches those
/// subscriptions before reading again, and dropping the queue detaches whatever is left.
#[derive(Default)]
pub(crate) struct BindingQueue {
    pending: RefCell<Vec<BindingId>>,
    waker: RefCell<Option<Rc<dyn Fn()>>>,
    sources: RefCell<Vec<Vec<SignalKey>>>,
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

    fn record(&self, binding: BindingId, index: u32, generation: u32) {
        let mut sources = self.sources.borrow_mut();
        let slot = binding.0 as usize;
        if sources.len() <= slot {
            sources.resize_with(slot + 1, Vec::new);
        }
        let signals = &mut sources[slot];
        if signals
            .iter()
            .any(|source| source.index == index && source.generation == generation)
        {
            return;
        }
        signals.push(SignalKey { index, generation });
    }

    fn take_sources(&self, binding: BindingId) -> Vec<SignalKey> {
        let mut sources = self.sources.borrow_mut();
        sources
            .get_mut(binding.0 as usize)
            .map(std::mem::take)
            .unwrap_or_default()
    }
}

impl Drop for BindingQueue {
    fn drop(&mut self) {
        let sources = std::mem::take(&mut *self.sources.borrow_mut());
        let queue = self as *const Self;
        RUNTIME.with(|runtime| {
            for (binding, signals) in sources.into_iter().enumerate() {
                let binding = BindingId(u32::try_from(binding).expect("too many bindings"));
                for source in signals {
                    runtime.remove_binding(source.index, source.generation, queue, binding);
                }
            }
        });
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
    /// Created for a component prop default. A setter disposes it when another handle replaces it.
    ephemeral: bool,
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
    fn create(&self, value: Box<dyn Any>, ephemeral: bool) -> (u32, u32) {
        let generation = {
            let mut next = self.next_generation.borrow_mut();
            *next = next.wrapping_add(1);
            *next
        };
        let slot = SignalSlot {
            generation,
            ephemeral,
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
        let value = self.with_slot(index, generation, |slot| {
            if let Some(observer) = observer.clone()
                && !slot
                    .subscribers
                    .iter()
                    .any(|existing| existing.same_as(&observer))
            {
                slot.subscribers.push(observer);
            }
            slot.value.clone()
        });
        if let Some(observer) = observer
            && let Some(queue) = observer.queue.upgrade()
        {
            queue.record(observer.binding, index, generation);
        }
        value
    }

    fn remove_binding(
        &self,
        index: u32,
        generation: u32,
        queue: *const BindingQueue,
        binding: BindingId,
    ) {
        let mut slots = self.slots.borrow_mut();
        let Some(slot) = slots.get_mut(index as usize).and_then(Option::as_mut) else {
            return;
        };
        if slot.generation != generation {
            return;
        }
        slot.subscribers
            .retain(|existing| existing.binding != binding || existing.queue.as_ptr() != queue);
    }

    fn dispose_if_placeholder(&self, index: u32, generation: u32) {
        let ephemeral = self
            .slots
            .borrow()
            .get(index as usize)
            .and_then(Option::as_ref)
            .is_some_and(|slot| slot.generation == generation && slot.ephemeral);
        if ephemeral {
            self.dispose(index, generation);
        }
    }

    fn hold(&self, index: u32, generation: u32) {
        let mut slots = self.slots.borrow_mut();
        let Some(slot) = slots.get_mut(index as usize).and_then(Option::as_mut) else {
            return;
        };
        if slot.generation == generation {
            slot.ephemeral = false;
        }
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

/// Drops the subscriptions this binding recorded last time, before `scope` reads again.
fn detach(observer: &Observer) {
    let Some(queue) = observer.queue.upgrade() else {
        return;
    };
    detach_binding(queue.as_ref(), observer.binding);
}

/// Drops every subscription `binding` recorded. A second call does nothing.
pub(crate) fn detach_binding(queue: &BindingQueue, binding: BindingId) {
    let sources = queue.take_sources(binding);
    let queue = queue as *const BindingQueue;
    RUNTIME.with(|runtime| {
        for source in sources {
            runtime.remove_binding(source.index, source.generation, queue, binding);
        }
    });
}

/// Signals created while this owner is the innermost one. Disposing the owner frees them.
#[derive(Clone, Copy)]
pub(crate) struct OwnerId {
    index: u32,
    generation: u32,
}

struct MountOwner {
    generation: u32,
    signals: Vec<SignalKey>,
}

thread_local! {
    static OWNERS: RefCell<Vec<Option<MountOwner>>> = const { RefCell::new(Vec::new()) };
    static OWNER_FREE: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static OWNER_STACK: RefCell<Vec<OwnerId>> = const { RefCell::new(Vec::new()) };
    static OWNER_GENERATION: Cell<u32> = const { Cell::new(0) };
}

/// Pushes an owner. Signals created before [`OwnerGuard`] drops are recorded on it.
pub(crate) fn enter_owner() -> OwnerGuard {
    let generation = OWNER_GENERATION.with(|next| {
        let generation = next.get().wrapping_add(1);
        next.set(generation);
        generation
    });
    let index = OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        let owner = MountOwner {
            generation,
            signals: Vec::new(),
        };
        match OWNER_FREE.with(|free| free.borrow_mut().pop()) {
            Some(index) => {
                owners[index as usize] = Some(owner);
                index
            }
            None => {
                owners.push(Some(owner));
                u32::try_from(owners.len() - 1).expect("too many owners")
            }
        }
    });
    let id = OwnerId { index, generation };
    OWNER_STACK.with(|stack| stack.borrow_mut().push(id));
    OwnerGuard(id)
}

fn exit_owner(id: OwnerId) {
    OWNER_STACK.with(|stack| {
        let popped = stack.borrow_mut().pop();
        debug_assert!(popped == Some(id));
    });
}

/// Frees every signal this owner still holds. A second call, or a stale id, does nothing.
pub(crate) fn dispose_owner(id: OwnerId) {
    let signals = OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        let slot = owners.get_mut(id.index as usize)?;
        if slot.as_ref().is_some_and(|owner| owner.generation != id.generation) {
            return None;
        }
        slot.take().map(|owner| owner.signals)
    });
    let Some(signals) = signals else {
        return;
    };
    OWNER_FREE.with(|free| free.borrow_mut().push(id.index));
    RUNTIME.with(|runtime| {
        for signal in signals {
            runtime.dispose(signal.index, signal.generation);
        }
    });
}

fn record_signal(index: u32, generation: u32) {
    let Some(owner) = OWNER_STACK.with(|stack| stack.borrow().last().copied()) else {
        return;
    };
    OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if let Some(Some(slot)) = owners.get_mut(owner.index as usize)
            && slot.generation == owner.generation
        {
            slot.signals.push(SignalKey { index, generation });
        }
    });
}

/// Pops the owner entered by [`enter_owner`] when dropped. The recorded signals stay until
/// [`dispose_owner`].
pub(crate) struct OwnerGuard(OwnerId);

impl OwnerGuard {
    pub(crate) fn id(&self) -> OwnerId {
        self.0
    }
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        exit_owner(self.0);
    }
}

impl PartialEq for OwnerId {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

struct RestoreObserver(Option<Observer>);

impl Drop for RestoreObserver {
    fn drop(&mut self) {
        let previous = self.0.take();
        RUNTIME.with(|runtime| *runtime.current_observer.borrow_mut() = previous);
    }
}

/// Runs `scope` with `observer` as the current subscriber, restoring the previous one afterwards.
///
/// Subscriptions from the previous run of this binding are removed first. Reads inside `scope`
/// subscribe again, so a signal the binding no longer reads loses this observer.
pub(crate) fn track<R>(observer: Observer, scope: impl FnOnce() -> R) -> R {
    detach(&observer);
    let previous = RUNTIME.with(|runtime| runtime.current_observer.replace(Some(observer)));
    let _restore = RestoreObserver(previous);
    scope()
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
        Self::create(value, false)
    }

    /// A prop default. The component setter disposes it when another handle replaces it.
    fn placeholder(value: T) -> Self {
        Self::create(value, true)
    }

    fn create(value: T, ephemeral: bool) -> Self {
        let (index, generation) =
            RUNTIME.with(|runtime| runtime.create(Box::new(value), ephemeral));
        record_signal(index, generation);
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
    ///
    /// A second call on any copy is a no-op: the slot is already on the free list, and a new
    /// signal that reused the index has a different generation.
    pub fn dispose(self) {
        RUNTIME.with(|runtime| runtime.dispose(self.index, self.generation));
    }

    fn dispose_if_placeholder(self) {
        RUNTIME.with(|runtime| runtime.dispose_if_placeholder(self.index, self.generation));
    }

    fn hold(self) {
        RUNTIME.with(|runtime| runtime.hold(self.index, self.generation));
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
    /// A component prop default. The setter disposes this signal when another handle replaces it.
    /// [`ByValue`](crate::ByValue) keeps the signal and clears that mark.
    #[must_use]
    pub fn placeholder(value: T) -> Self {
        Self {
            inner: Signal::placeholder(value),
        }
    }

    /// Frees this signal when it is still a [`placeholder`](Self::placeholder).
    pub fn dispose_if_placeholder(self) {
        self.inner.dispose_if_placeholder();
    }

    pub(crate) fn hold(&self) {
        self.inner.hold();
    }

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

    /// Prop default. Primitives, [`String`] and [`Option`] allocate a placeholder the setter can
    /// dispose when it switches the prop to another handle. Other values use [`into_reactive`](Self::into_reactive).
    fn into_prop_default(self) -> Self::Output
    where
        Self: Sized,
    {
        self.into_reactive()
    }
}

/// Drops a prop placeholder after a setter replaces it with a different handle.
pub trait RetirePlaceholder {
    fn dispose_if_placeholder(self);
}

impl<T: 'static> RetirePlaceholder for Reactive<T> {
    fn dispose_if_placeholder(self) {
        Reactive::dispose_if_placeholder(self);
    }
}

/// Tracks `value`.
///
/// A primitive, [`String`], [`Option`] or [`Vec`] becomes one [`Reactive`] handle. A `#[reactive]`
/// struct is already tracked, so it is returned as itself. [`Reactive`] and [`Signal`] become a
/// [`Reactive`] handle without wrapping them again.
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

                fn into_prop_default(self) -> Self::Output {
                    Reactive::placeholder(self)
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

    fn into_prop_default(self) -> Self::Output {
        Reactive::placeholder(self)
    }
}

impl<T: 'static> IntoReactive for Vec<T> {
    type Output = Reactive<Vec<T>>;

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

    fn into_prop_default(self) -> Self::Output {
        Reactive::placeholder(self)
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
pub(crate) fn live_signal_count() -> usize {
    RUNTIME.with(|runtime| {
        runtime
            .slots
            .borrow()
            .iter()
            .filter(|slot| slot.is_some())
            .count()
    })
}

#[cfg(test)]
pub(crate) fn signal_is_alive<T: 'static>(signal: Signal<T>) -> bool {
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
pub(crate) fn signal_subscriber_count<T: 'static>(signal: Signal<T>) -> usize {
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
    fn owner_disposes_only_signals_created_inside_it() {
        let before = live_signal_count();
        let outer = signal(1);
        let guard = enter_owner();
        let inner = signal(2);
        let owner = guard.id();
        drop(guard);
        assert!(signal_is_alive(inner));
        dispose_owner(owner);
        dispose_owner(owner);
        assert!(!signal_is_alive(inner));
        assert!(signal_is_alive(outer));
        assert_eq!(outer.get(), 1);
        assert_eq!(live_signal_count(), before + 1);
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

        let items = reactive(vec![1, 2]);
        items.update(|list| list.push(3));
        assert_eq!(items.with(|list| list.clone()), vec![1, 2, 3]);
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

    fn occupied_slots() -> usize {
        RUNTIME.with(|runtime| {
            runtime
                .slots
                .borrow()
                .iter()
                .filter(|slot| slot.is_some())
                .count()
        })
    }

    fn signal_alive<T: 'static>(signal: Signal<T>) -> bool {
        RUNTIME.with(|runtime| {
            runtime
                .slots
                .borrow()
                .get(signal.index as usize)
                .and_then(|slot| slot.as_ref())
                .is_some_and(|slot| slot.generation == signal.generation)
        })
    }

    fn subscriber_count<T: 'static>(signal: Signal<T>) -> usize {
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

    /// The component setter: by-value writes through, by-handle replaces and retires a placeholder.
    fn apply_prop<T, Value, Marker>(slot: &mut Reactive<T>, value: Value)
    where
        T: 'static + PartialEq,
        Value: crate::PropInput<T, Marker>,
        Reactive<T>: PartialEq,
    {
        let previous = *slot;
        value.apply(slot);
        if previous != *slot {
            previous.dispose_if_placeholder();
        }
    }

    #[test]
    fn binding_drops_signals_it_stops_reading() {
        let binding_queue = queue();
        let flag = signal(true);
        let left = signal(1);
        let right = signal(2);
        let run = || {
            track(Observer::new(&binding_queue, BindingId(1)), || {
                if flag.get() { left.get() } else { right.get() }
            });
        };
        run();
        assert_eq!(subscriber_count(flag), 1);
        assert_eq!(subscriber_count(left), 1);
        assert_eq!(subscriber_count(right), 0);
        flag.set(false);
        run();
        assert_eq!(subscriber_count(flag), 1);
        assert_eq!(subscriber_count(left), 0);
        assert_eq!(subscriber_count(right), 1);
    }

    #[test]
    fn dropping_the_queue_detaches_subscribers() {
        let watched = signal(0);
        {
            let binding_queue = queue();
            track(Observer::new(&binding_queue, BindingId(7)), || {
                watched.get()
            });
            assert_eq!(subscriber_count(watched), 1);
        }
        assert_eq!(subscriber_count(watched), 0);
    }

    #[test]
    fn placeholder_replaced_by_a_handle_returns_its_slot() {
        let before = occupied_slots();
        let mut slot = Reactive::placeholder(String::new());
        let placeholder = slot.inner;
        let parent = reactive(String::from("parent"));
        apply_prop(&mut slot, parent);
        assert_eq!(slot, parent);
        assert!(!signal_alive(placeholder));
        assert!(signal_alive(parent.inner));
        assert_eq!(occupied_slots(), before + 1);

        let reused = signal(String::from("next"));
        assert_eq!(reused.index, placeholder.index);
        assert_ne!(reused.generation, placeholder.generation);
    }

    #[test]
    fn by_value_keeps_the_placeholder_slot() {
        let mut slot = Reactive::placeholder(String::new());
        let placeholder = slot.inner;
        apply_prop(&mut slot, "once");
        assert_eq!(slot.get(), "once");
        assert!(signal_alive(placeholder));

        let parent = reactive(String::from("parent"));
        apply_prop(&mut slot, parent);
        assert_eq!(slot, parent);
        assert!(signal_alive(placeholder));
        assert!(signal_alive(parent.inner));
    }

    #[test]
    fn disposing_a_signal_twice_is_a_no_op() {
        let value = signal(1);
        let before = occupied_slots();
        value.dispose();
        value.dispose();
        assert!(!signal_alive(value));
        assert_eq!(occupied_slots(), before - 1);
    }

    #[test]
    fn ui_new_disposes_its_theme_signal() {
        use crate::{Theme, Ui, div};

        let before = occupied_slots();
        let theme = {
            let ui = Ui::new(div(), Theme::light());
            let theme = ui.theme_signal();
            assert!(signal_alive(theme));
            assert_eq!(occupied_slots(), before + 1);
            theme
        };
        assert!(!signal_alive(theme));
        assert_eq!(occupied_slots(), before);
    }

    #[test]
    fn with_theme_signal_keeps_the_caller_signal() {
        use std::sync::Arc;

        use crate::{Theme, Ui, div};

        let theme = signal(Arc::new(Theme::light()));
        let before = occupied_slots();
        drop(Ui::with_theme_signal(div(), theme));
        assert!(signal_alive(theme));
        assert_eq!(occupied_slots(), before);
    }

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
