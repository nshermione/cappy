//! Fine-grained reactive signals.
//!
//! A [`Signal`] is a [`Clone`] handle to a value stored in a [`crate::Context`] runtime. Reading a
//! signal inside a tracked scope subscribes that scope; writing the signal queues every subscriber
//! so the UI re-evaluates only the bindings that depend on it.
//!
//! [`Context::reactive`] turns a primitive into a [`Reactive`] handle. `#[store]` on a struct keeps the
//! struct's name and tracks each field. A value that is already reactive is returned as itself.
//! `cx.use_store(Type)` returns the one value of that struct stored in the context.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

/// Identifies one reactive binding inside a [`BindingQueue`].
///
/// `generation` changes when the slot is recycled, so a notification queued for a disposed
/// binding does not run the binding that reused the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct BindingId {
    index: u32,
    generation: u32,
}

impl BindingId {
    pub(crate) fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Binding `index` at generation 0. Tests use this when no slot is allocated.
    pub(crate) const fn from_index(index: u32) -> Self {
        Self {
            index,
            generation: 0,
        }
    }

    pub(crate) fn slot(self) -> usize {
        self.index as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

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
pub(crate) struct BindingQueue {
    pub(crate) runtime: Rc<Runtime>,
    pending: RefCell<Vec<BindingId>>,
    waker: RefCell<Option<Rc<dyn Fn()>>>,
    sources: RefCell<Vec<Vec<SignalKey>>>,
}

impl BindingQueue {
    pub(crate) fn new(runtime: Rc<Runtime>) -> Self {
        Self {
            runtime,
            pending: RefCell::new(Vec::new()),
            waker: RefCell::new(None),
            sources: RefCell::new(Vec::new()),
        }
    }

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
        let slot = binding.slot();
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
            .get_mut(binding.slot())
            .map(std::mem::take)
            .unwrap_or_default()
    }

    pub(crate) fn source_slots(&self) -> usize {
        self.sources.borrow().len()
    }

    pub(crate) fn source_capacity(&self) -> usize {
        self.sources.borrow().capacity()
    }

    pub(crate) fn subscription_count(&self) -> usize {
        self.sources
            .borrow()
            .iter()
            .map(|sources| sources.len())
            .sum()
    }

    /// Drops source slots past `len` and returns their spare capacity to the allocator.
    pub(crate) fn shrink_sources(&self, len: usize) {
        let mut sources = self.sources.borrow_mut();
        if sources.len() > len {
            sources.truncate(len);
        }
        sources.shrink_to_fit();
    }
}

impl Drop for BindingQueue {
    fn drop(&mut self) {
        let sources = std::mem::take(&mut *self.sources.borrow_mut());
        let queue = self as *const Self;
        for (binding, signals) in sources.into_iter().enumerate() {
            for source in signals {
                self.runtime.detach_queue_slot(
                    source.index,
                    source.generation,
                    queue,
                    binding as u32,
                );
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
    /// Created for a component prop default. A setter disposes it when another handle replaces it.
    ephemeral: bool,
    value: SharedValue,
    subscribers: Vec<Observer>,
}

/// One listener closure. The handle in [`crate::Listener`] only stores the slot id.
pub(crate) struct ListenerSlot {
    pub(crate) generation: u32,
    pub(crate) handler: Option<Box<dyn Any>>,
}

struct MountOwner {
    generation: u32,
    signals: Vec<SignalKey>,
}

#[derive(Default)]
pub(crate) struct Runtime {
    slots: RefCell<Vec<Option<SignalSlot>>>,
    free_slots: RefCell<Vec<u32>>,
    next_generation: RefCell<u32>,
    pub(crate) current_observer: RefCell<Option<Observer>>,
    pub(crate) listeners: RefCell<Vec<Option<ListenerSlot>>>,
    pub(crate) next_listener_generation: Cell<u32>,
    owners: RefCell<Vec<Option<MountOwner>>>,
    free_owners: RefCell<Vec<u32>>,
    owner_stack: RefCell<Vec<OwnerId>>,
    next_owner_generation: Cell<u32>,
    singletons: RefCell<HashMap<TypeId, Box<dyn Any>>>,
    pub(crate) components: RefCell<Vec<Rc<dyn Any>>>,
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

    /// Drops every subscriber from `queue` on this signal whose binding uses `binding_slot`,
    /// whatever generation it had. Used when the queue itself is going away.
    fn detach_queue_slot(
        &self,
        index: u32,
        generation: u32,
        queue: *const BindingQueue,
        binding_slot: u32,
    ) {
        let mut slots = self.slots.borrow_mut();
        let Some(slot) = slots.get_mut(index as usize).and_then(Option::as_mut) else {
            return;
        };
        if slot.generation != generation {
            return;
        }
        slot.subscribers.retain(|existing| {
            existing.binding.slot() != binding_slot as usize || existing.queue.as_ptr() != queue
        });
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

    fn enter_owner(self: &Rc<Self>) -> OwnerGuard {
        let generation = {
            let next = &self.next_owner_generation;
            let generation = next.get().wrapping_add(1);
            next.set(generation);
            generation
        };
        let index = {
            let mut owners = self.owners.borrow_mut();
            let owner = MountOwner {
                generation,
                signals: Vec::new(),
            };
            match self.free_owners.borrow_mut().pop() {
                Some(index) => {
                    owners[index as usize] = Some(owner);
                    index
                }
                None => {
                    owners.push(Some(owner));
                    u32::try_from(owners.len() - 1).expect("too many owners")
                }
            }
        };
        let id = OwnerId { index, generation };
        self.owner_stack.borrow_mut().push(id);
        OwnerGuard {
            runtime: Rc::clone(self),
            id,
        }
    }

    fn exit_owner(&self, id: OwnerId) {
        let popped = self.owner_stack.borrow_mut().pop();
        debug_assert!(popped == Some(id));
    }

    fn dispose_owner(&self, id: OwnerId) {
        let signals = {
            let mut owners = self.owners.borrow_mut();
            let Some(slot) = owners.get_mut(id.index as usize) else {
                return;
            };
            if slot.as_ref().is_some_and(|owner| owner.generation != id.generation) {
                return;
            }
            let Some(owner) = slot.take() else {
                return;
            };
            self.free_owners.borrow_mut().push(id.index);
            owner.signals
        };
        for signal in signals {
            self.dispose(signal.index, signal.generation);
        }
    }

    fn record_signal(&self, index: u32, generation: u32) {
        let Some(owner) = self.owner_stack.borrow().last().copied() else {
            return;
        };
        let mut owners = self.owners.borrow_mut();
        if let Some(Some(slot)) = owners.get_mut(owner.index as usize)
            && slot.generation == owner.generation
        {
            slot.signals.push(SignalKey { index, generation });
        }
    }

    pub(crate) fn push_component<T: Clone + 'static>(&self, component: &T) {
        self.components
            .borrow_mut()
            .push(Rc::new(component.clone()));
    }

    pub(crate) fn pop_component(&self) {
        self.components.borrow_mut().pop();
    }

    pub(crate) fn singleton<T: Clone + 'static>(&self, create: impl FnOnce() -> T) -> T {
        let id = TypeId::of::<T>();
        if let Some(existing) = self.singletons.borrow().get(&id) {
            return existing
                .downcast_ref::<T>()
                .expect("singleton stored under its own type")
                .clone();
        }
        let created = create();
        let mut singletons = self.singletons.borrow_mut();
        if let Some(existing) = singletons.get(&id) {
            return existing
                .downcast_ref::<T>()
                .expect("singleton stored under its own type")
                .clone();
        }
        singletons.insert(id, Box::new(created.clone()));
        created
    }
}

/// Drops every subscription `binding` recorded. A second call does nothing.
pub(crate) fn detach_binding(queue: &BindingQueue, binding: BindingId) {
    let sources = queue.take_sources(binding);
    let queue_ptr = queue as *const BindingQueue;
    for source in sources {
        queue
            .runtime
            .remove_binding(source.index, source.generation, queue_ptr, binding);
    }
}

/// Signals created while this owner is the innermost one. Disposing the owner frees them.
#[derive(Clone, Copy)]
pub(crate) struct OwnerId {
    index: u32,
    generation: u32,
}

/// Pushes an owner. Signals created before [`OwnerGuard`] drops are recorded on it.
pub(crate) fn enter_owner(runtime: &Rc<Runtime>) -> OwnerGuard {
    runtime.enter_owner()
}

/// Frees every signal this owner still holds. A second call, or a stale id, does nothing.
pub(crate) fn dispose_owner(runtime: &Runtime, id: OwnerId) {
    runtime.dispose_owner(id);
}

/// Pops the owner entered by [`enter_owner`] when dropped. The recorded signals stay until
/// [`dispose_owner`].
pub(crate) struct OwnerGuard {
    runtime: Rc<Runtime>,
    id: OwnerId,
}

impl OwnerGuard {
    pub(crate) fn id(&self) -> OwnerId {
        self.id
    }
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        self.runtime.exit_owner(self.id);
    }
}

impl PartialEq for OwnerId {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

struct RestoreObserver {
    runtime: Rc<Runtime>,
    previous: Option<Observer>,
}

impl Drop for RestoreObserver {
    fn drop(&mut self) {
        *self.runtime.current_observer.borrow_mut() = self.previous.take();
    }
}

/// Runs `scope` with `observer` as the current subscriber, restoring the previous one afterwards.
///
/// Subscriptions from the previous run of this binding are removed first. Reads inside `scope`
/// subscribe again, so a signal the binding no longer reads loses this observer.
pub(crate) fn track<R>(observer: Observer, scope: impl FnOnce() -> R) -> R {
    let queue = observer
        .queue
        .upgrade()
        .expect("binding queue dropped before its binding ran");
    detach_binding(queue.as_ref(), observer.binding);
    let runtime = Rc::clone(&queue.runtime);
    drop(queue);
    let previous = runtime.current_observer.borrow_mut().replace(observer);
    let _restore = RestoreObserver { runtime, previous };
    scope()
}

/// A reactive value. Cloning clones the runtime handle; every clone refers to the same value.
///
/// The value lives until [`Signal::dispose`] or until the last [`crate::Context`] and handle drop.
pub struct Signal<T: 'static> {
    runtime: Rc<Runtime>,
    index: u32,
    generation: u32,
    _value: PhantomData<fn() -> T>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            runtime: Rc::clone(&self.runtime),
            index: self.index,
            generation: self.generation,
            _value: PhantomData,
        }
    }
}

impl<T: 'static> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.runtime, &other.runtime)
            && self.index == other.index
            && self.generation == other.generation
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

impl<T: 'static> Signal<T> {
    pub(crate) fn create(runtime: &Rc<Runtime>, value: T, ephemeral: bool) -> Self {
        let (index, generation) = runtime.create(Box::new(value), ephemeral);
        runtime.record_signal(index, generation);
        Self {
            runtime: Rc::clone(runtime),
            index,
            generation,
            _value: PhantomData,
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
        let value = self.runtime.read(self.index, self.generation, false);
        {
            let mut value = value.borrow_mut();
            change(value.downcast_mut::<T>().expect(DEAD_SIGNAL));
        }
        let subscribers = self.runtime.subscribers(self.index, self.generation);
        for subscriber in subscribers {
            subscriber.notify();
        }
    }

    /// Frees the value. Using any clone of this signal afterwards panics.
    ///
    /// A second call on any clone is a no-op: the slot is already on the free list, and a new
    /// signal that reused the index has a different generation.
    pub fn dispose(self) {
        self.runtime.dispose(self.index, self.generation);
    }

    fn dispose_if_placeholder(self) {
        self.runtime
            .dispose_if_placeholder(self.index, self.generation);
    }

    fn hold(self) {
        self.runtime.hold(self.index, self.generation);
    }

    fn read_value<R>(&self, track: bool, read: impl FnOnce(&T) -> R) -> R {
        let value = self.runtime.read(self.index, self.generation, track);
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

/// A [`Clone`] handle whose reads subscribe the current tracking scope.
///
/// Built by [`Context::reactive`]. Formatting and `==` against the inner value read it. [`Reactive::set`]
/// and [`Reactive::update`] write it. The handle does not implement [`Deref`](std::ops::Deref):
/// the value lives in the context runtime, not inside the handle.
pub struct Reactive<T: 'static> {
    inner: Signal<T>,
}

impl<T: 'static> Clone for Reactive<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

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
    pub(crate) fn placeholder(runtime: &Rc<Runtime>, value: T) -> Self {
        Self {
            inner: Signal::create(runtime, value, true),
        }
    }

    /// Frees this signal when it is still a [`placeholder`](Self::placeholder).
    pub fn dispose_if_placeholder(self) {
        self.inner.dispose_if_placeholder();
    }

    pub(crate) fn hold(&self) {
        self.inner.clone().hold();
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

/// A value [`Context::reactive`] can turn into a tracked handle, or leave unchanged when it already is one.
pub trait IntoReactive {
    /// The tracked form of this value.
    type Output;

    /// Converts this value into its tracked form.
    fn into_reactive(self, context: &crate::Context) -> Self::Output;

    /// Prop default. Primitives, [`String`] and [`Option`] allocate a placeholder the setter can
    /// dispose when it switches the prop to another handle. Other values use [`into_reactive`](Self::into_reactive).
    fn into_prop_default(self, context: &crate::Context) -> Self::Output
    where
        Self: Sized,
    {
        self.into_reactive(context)
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

impl crate::Context {
    /// Allocates a signal in this context.
    pub fn signal<T: 'static>(&self, value: T) -> Signal<T> {
        Signal::create(&self.runtime, value, false)
    }

    /// Tracks `value` in this context.
    ///
    /// A primitive, [`String`], [`Option`] or [`Vec`] becomes one [`Reactive`] handle. A
    /// `#[store]` struct is already tracked, so it is returned as itself. [`Reactive`] and
    /// [`Signal`] become a [`Reactive`] handle without wrapping them again.
    pub fn reactive<T: IntoReactive>(&self, value: T) -> T::Output {
        value.into_reactive(self)
    }

    /// Returns the one `T` stored for this context, creating it on the first call.
    ///
    /// Pass the marker `#[store]` emits under the struct's name: `cx.use_store(Counter)`.
    pub fn use_store<T: crate::Singleton>(&self, _of: crate::SingletonOf<T>) -> T {
        self.runtime.singleton(|| T::create(self))
    }
}

macro_rules! primitive_reactive {
    ($($ty:ty),* $(,)?) => {
        $(
            impl IntoReactive for $ty {
                type Output = Reactive<$ty>;

                fn into_reactive(self, context: &crate::Context) -> Self::Output {
                    Reactive {
                        inner: context.signal(self),
                    }
                }

                fn into_prop_default(self, context: &crate::Context) -> Self::Output {
                    Reactive::placeholder(&context.runtime, self)
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

    fn into_reactive(self, context: &crate::Context) -> Self::Output {
        Reactive {
            inner: context.signal(self),
        }
    }

    fn into_prop_default(self, context: &crate::Context) -> Self::Output {
        Reactive::placeholder(&context.runtime, self)
    }
}

impl<T: 'static> IntoReactive for Vec<T> {
    type Output = Reactive<Vec<T>>;

    fn into_reactive(self, context: &crate::Context) -> Self::Output {
        Reactive {
            inner: context.signal(self),
        }
    }
}

impl<T: 'static> IntoReactive for Option<T> {
    type Output = Reactive<Option<T>>;

    fn into_reactive(self, context: &crate::Context) -> Self::Output {
        Reactive {
            inner: context.signal(self),
        }
    }

    fn into_prop_default(self, context: &crate::Context) -> Self::Output {
        Reactive::placeholder(&context.runtime, self)
    }
}

impl<T: 'static> IntoReactive for Reactive<T> {
    type Output = Self;

    fn into_reactive(self, _context: &crate::Context) -> Self::Output {
        self
    }
}

impl<T> crate::FieldInit for Reactive<T>
where
    T: Default + IntoReactive<Output = Reactive<T>> + 'static,
{
    fn field_init(cx: &crate::Context) -> Self {
        T::default().into_reactive(cx)
    }
}

impl<T: 'static> IntoReactive for Signal<T> {
    type Output = Reactive<T>;

    fn into_reactive(self, _context: &crate::Context) -> Self::Output {
        Reactive { inner: self }
    }
}

/// Drops signal and owner slots that nothing holds anymore, when they sit at the end of the arena.
pub(crate) fn reclaim_unused_slots(runtime: &Runtime) {
    let len = {
        let mut slots = runtime.slots.borrow_mut();
        while matches!(slots.last(), Some(None)) {
            slots.pop();
        }
        slots.shrink_to_fit();
        slots.len() as u32
    };
    let mut free = runtime.free_slots.borrow_mut();
    free.retain(|index| *index < len);
    free.shrink_to_fit();
    let len = {
        let mut owners = runtime.owners.borrow_mut();
        while matches!(owners.last(), Some(None)) {
            owners.pop();
        }
        owners.shrink_to_fit();
        owners.len() as u32
    };
    let mut free = runtime.free_owners.borrow_mut();
    free.retain(|index| *index < len);
    free.shrink_to_fit();
}

pub(crate) struct RuntimeCensus {
    pub signal_slots: usize,
    pub signal_capacity: usize,
    pub live_signals: usize,
    pub signal_subscribers: usize,
    pub owners: usize,
    pub owner_capacity: usize,
    pub live_owners: usize,
    pub listeners: usize,
    pub listener_capacity: usize,
    pub live_listeners: usize,
    pub singletons: usize,
}

pub(crate) fn runtime_census(runtime: &Runtime) -> RuntimeCensus {
    let slots = runtime.slots.borrow();
    let live_signals = slots.iter().filter(|slot| slot.is_some()).count();
    let signal_subscribers = slots
        .iter()
        .flatten()
        .map(|slot| slot.subscribers.len())
        .sum();
    let owners = runtime.owners.borrow();
    let listeners = runtime.listeners.borrow();
    RuntimeCensus {
        signal_slots: slots.len(),
        signal_capacity: slots.capacity(),
        live_signals,
        signal_subscribers,
        owners: owners.len(),
        owner_capacity: owners.capacity(),
        live_owners: owners.iter().filter(|owner| owner.is_some()).count(),
        listeners: listeners.len(),
        listener_capacity: listeners.capacity(),
        live_listeners: listeners.iter().filter(|slot| slot.is_some()).count(),
        singletons: runtime.singletons.borrow().len(),
    }
}

#[cfg(test)]
pub(crate) fn live_signal_count(runtime: &Runtime) -> usize {
    runtime
        .slots
        .borrow()
        .iter()
        .filter(|slot| slot.is_some())
        .count()
}

#[cfg(test)]
pub(crate) fn signal_is_alive<T: 'static>(signal: &Signal<T>) -> bool {
    signal
        .runtime
        .slots
        .borrow()
        .get(signal.index as usize)
        .and_then(|slot| slot.as_ref())
        .is_some_and(|slot| slot.generation == signal.generation)
}

#[cfg(test)]
pub(crate) fn signal_subscriber_count<T: 'static>(signal: &Signal<T>) -> usize {
    signal
        .runtime
        .slots
        .borrow()
        .get(signal.index as usize)
        .and_then(|slot| slot.as_ref())
        .filter(|slot| slot.generation == signal.generation)
        .map(|slot| slot.subscribers.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn context() -> crate::Context {
        crate::Application::new().context().clone()
    }

    fn session() -> (crate::Context, Rc<BindingQueue>) {
        let context = context();
        let queue = Rc::new(BindingQueue::new(Rc::clone(&context.runtime)));
        (context, queue)
    }

    #[test]
    fn get_set_update_round_trip() {
        let count = context().signal(1);
        count.set(5);
        count.update(|value| *value += 1);
        assert_eq!(count.get(), 6);
    }

    #[test]
    fn clones_share_the_same_value() {
        let name = context().signal(String::from("a"));
        let copy = name.clone();
        copy.update(|value| value.push('b'));
        assert_eq!(name.get(), "ab");
    }

    #[test]
    fn tracked_reads_subscribe_and_writes_notify() {
        let (context, queue) = session();
        let count = context.signal(0);
        track(Observer::new(&queue, BindingId::from_index(3)), || count.get());
        assert!(queue.is_empty());
        count.set(1);
        assert_eq!(queue.take(), vec![BindingId::from_index(3)]);
    }

    #[test]
    fn untracked_reads_do_not_subscribe() {
        let (context, queue) = session();
        let count = context.signal(0);
        track(Observer::new(&queue, BindingId::from_index(1)), || {
            count.get_untracked()
        });
        count.set(1);
        assert!(queue.is_empty());
    }

    #[test]
    fn repeated_tracking_subscribes_once() {
        let (context, queue) = session();
        let count = context.signal(0);
        for _ in 0..3 {
            track(Observer::new(&queue, BindingId::from_index(1)), || count.get());
        }
        count.set(1);
        count.set(2);
        assert_eq!(queue.take(), vec![BindingId::from_index(1)]);
    }

    #[test]
    fn nested_scopes_restore_the_outer_observer() {
        let (context, queue) = session();
        let outer = context.signal(0);
        let inner = context.signal(0);
        track(Observer::new(&queue, BindingId::from_index(1)), || {
            track(Observer::new(&queue, BindingId::from_index(2)), || inner.get());
            outer.get()
        });
        outer.set(1);
        assert_eq!(queue.take(), vec![BindingId::from_index(1)]);
        inner.set(1);
        assert_eq!(queue.take(), vec![BindingId::from_index(2)]);
    }

    #[test]
    fn dropped_queue_stops_receiving() {
        let (context, _) = session();
        let count = context.signal(0);
        {
            let queue = Rc::new(BindingQueue::new(Rc::clone(&context.runtime)));
            track(Observer::new(&queue, BindingId::from_index(1)), || count.get());
        }
        count.set(1);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn waker_fires_once_per_batch() {
        let (context, queue) = session();
        let wakes = Rc::new(Cell::new(0));
        let counter = wakes.clone();
        queue.set_waker(Some(Rc::new(move || counter.set(counter.get() + 1))));
        let count = context.signal(0);
        track(Observer::new(&queue, BindingId::from_index(1)), || count.get());
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
        let count = context().signal(0);
        let reading = count.clone();
        count.dispose();
        reading.get();
    }

    #[test]
    fn disposed_slots_are_reused_without_aliasing() {
        let context = context();
        let first = context.signal(1);
        let kept = first.clone();
        first.dispose();
        let second = context.signal(2);
        assert_ne!(kept, second);
        assert_eq!(second.get(), 2);
    }

    #[test]
    fn owner_disposes_only_signals_created_inside_it() {
        let context = context();
        let before = live_signal_count(&context.runtime);
        let outer = context.signal(1);
        let guard = enter_owner(&context.runtime);
        let inner = context.signal(2);
        let owner = guard.id();
        drop(guard);
        assert!(signal_is_alive(&inner));
        dispose_owner(&context.runtime, owner);
        dispose_owner(&context.runtime, owner);
        assert!(!signal_is_alive(&inner));
        assert!(signal_is_alive(&outer));
        assert_eq!(outer.get(), 1);
        assert_eq!(live_signal_count(&context.runtime), before + 1);
    }

    #[test]
    fn reactive_primitives_read_and_write() {
        let context = context();
        let count = context.reactive(1_i64);
        assert_eq!(count.to_string(), "1");
        assert!(count == 1);
        count.set(2);
        assert_eq!(count.get(), 2);
        count.update(|value| *value += 1);
        assert_eq!(format!("{count}"), "3");

        let width = context.reactive(4_u32);
        assert_eq!(width.get(), 4);

        let selected = context.reactive(None::<usize>);
        selected.set(Some(1));

        let items = context.reactive(vec![1, 2]);
        items.update(|list| list.push(3));
        assert_eq!(items.with(|list| list.clone()), vec![1, 2, 3]);
        assert_eq!(selected.get(), Some(1));
    }

    #[test]
    fn reactive_keeps_an_existing_handle() {
        let context = context();
        let count = context.reactive(1_i64);
        assert_eq!(context.reactive(count.clone()), count);

        let inner = context.signal(String::from("hi"));
        let wrapped = context.reactive(inner.clone());
        wrapped.set(String::from("yo"));
        assert_eq!(inner.get(), "yo");
        assert_eq!(context.reactive(wrapped.clone()), wrapped);
    }

    /// The component setter: by-value writes through, by-handle replaces and retires a placeholder.
    fn apply_prop<T, Value, Marker>(slot: &mut Reactive<T>, value: Value)
    where
        T: 'static + PartialEq,
        Value: crate::PropInput<T, Marker>,
        Reactive<T>: PartialEq,
    {
        let previous = slot.clone();
        value.apply(slot);
        if previous != *slot {
            previous.dispose_if_placeholder();
        }
    }

    #[test]
    fn binding_drops_signals_it_stops_reading() {
        let (context, binding_queue) = session();
        let flag = context.signal(true);
        let left = context.signal(1);
        let right = context.signal(2);
        let run = || {
            track(Observer::new(&binding_queue, BindingId::from_index(1)), || {
                if flag.get() { left.get() } else { right.get() }
            });
        };
        run();
        assert_eq!(signal_subscriber_count(&flag), 1);
        assert_eq!(signal_subscriber_count(&left), 1);
        assert_eq!(signal_subscriber_count(&right), 0);
        flag.set(false);
        run();
        assert_eq!(signal_subscriber_count(&flag), 1);
        assert_eq!(signal_subscriber_count(&left), 0);
        assert_eq!(signal_subscriber_count(&right), 1);
    }

    #[test]
    fn dropping_the_queue_detaches_subscribers() {
        let (context, _) = session();
        let watched = context.signal(0);
        {
            let binding_queue = Rc::new(BindingQueue::new(Rc::clone(&context.runtime)));
            track(Observer::new(&binding_queue, BindingId::from_index(7)), || {
                watched.get()
            });
            assert_eq!(signal_subscriber_count(&watched), 1);
        }
        assert_eq!(signal_subscriber_count(&watched), 0);
    }

    #[test]
    fn placeholder_replaced_by_a_handle_returns_its_slot() {
        let context = context();
        let before = live_signal_count(&context.runtime);
        let mut slot = Reactive::placeholder(&context.runtime, String::new());
        let placeholder = slot.inner.clone();
        let parent = context.reactive(String::from("parent"));
        apply_prop(&mut slot, parent.clone());
        assert_eq!(slot, parent);
        assert!(!signal_is_alive(&placeholder));
        assert!(signal_is_alive(&parent.inner));
        assert_eq!(live_signal_count(&context.runtime), before + 1);

        let reused = context.signal(String::from("next"));
        assert_eq!(reused.index, placeholder.index);
        assert_ne!(reused.generation, placeholder.generation);
    }

    #[test]
    fn by_value_keeps_the_placeholder_slot() {
        let context = context();
        let mut slot = Reactive::placeholder(&context.runtime, String::new());
        let placeholder = slot.inner.clone();
        apply_prop(&mut slot, "once");
        assert_eq!(slot.get(), "once");
        assert!(signal_is_alive(&placeholder));

        let parent = context.reactive(String::from("parent"));
        apply_prop(&mut slot, parent.clone());
        assert_eq!(slot, parent);
        assert!(signal_is_alive(&placeholder));
        assert!(signal_is_alive(&parent.inner));
    }

    #[test]
    fn disposing_a_signal_twice_is_a_no_op() {
        let context = context();
        let value = context.signal(1);
        let again = value.clone();
        let probe = value.clone();
        let before = live_signal_count(&context.runtime);
        value.dispose();
        again.dispose();
        assert!(!signal_is_alive(&probe));
        assert_eq!(live_signal_count(&context.runtime), before - 1);
    }

    #[test]
    fn ui_new_disposes_its_theme_signal() {
        use crate::{Theme, Ui, div};

        let context = context();
        let before = live_signal_count(&context.runtime);
        let theme = {
            let ui = Ui::new(&context, div(), Theme::light());
            let theme = ui.theme_signal();
            assert!(signal_is_alive(&theme));
            assert_eq!(live_signal_count(&context.runtime), before + 1);
            theme
        };
        assert!(!signal_is_alive(&theme));
        assert_eq!(live_signal_count(&context.runtime), before);
    }

    #[test]
    fn with_theme_signal_keeps_the_caller_signal() {
        use std::sync::Arc;

        use crate::{Theme, Ui, div};

        let context = context();
        let theme = context.signal(Arc::new(Theme::light()));
        let before = live_signal_count(&context.runtime);
        drop(Ui::with_theme_signal(&context, div(), theme.clone()));
        assert!(signal_is_alive(&theme));
        assert_eq!(live_signal_count(&context.runtime), before);
    }

    #[test]
    fn display_read_tracks_in_a_binding() {
        use crate::{FrameInput, Theme, Ui, div, text_dyn};

        let context = context();
        let count = context.reactive(1_i64);
        let label_count = count.clone();
        let mut ui = Ui::new(
            &context,
            div().child(text_dyn(move || format!("{label_count}")).key("label")),
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
