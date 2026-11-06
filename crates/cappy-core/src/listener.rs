//! Copy handles for component events.
//!
//! The closure lives in a thread-local slot, like a signal's value. [`Listener`] only stores the
//! slot id, so a component that holds listeners stays [`Copy`]. Slots are not freed; the UI has no
//! unmount yet, and signals have the same limit.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;

struct ListenerSlot {
    generation: u32,
    handler: Option<Box<dyn Any>>,
}

#[derive(Default)]
struct Runtime {
    slots: RefCell<Vec<Option<ListenerSlot>>>,
    next_generation: RefCell<u32>,
}

thread_local! {
    static RUNTIME: Runtime = Runtime::default();
}

const DEAD_LISTENER: &str = "listener was used on a thread other than the one that created it";

impl Runtime {
    fn create(&self) -> (u32, u32) {
        let generation = {
            let mut next = self.next_generation.borrow_mut();
            *next = next.wrapping_add(1);
            *next
        };
        let mut slots = self.slots.borrow_mut();
        slots.push(Some(ListenerSlot {
            generation,
            handler: None,
        }));
        let index = u32::try_from(slots.len() - 1).expect("too many live listeners");
        (index, generation)
    }

    fn with_slot<R>(
        &self,
        index: u32,
        generation: u32,
        f: impl FnOnce(&mut ListenerSlot) -> R,
    ) -> R {
        let mut slots = self.slots.borrow_mut();
        let slot = slots
            .get_mut(index as usize)
            .and_then(Option::as_mut)
            .filter(|slot| slot.generation == generation)
            .expect(DEAD_LISTENER);
        f(slot)
    }

    fn store(&self, index: u32, generation: u32, handler: Box<dyn Any>) {
        self.with_slot(index, generation, |slot| slot.handler = Some(handler));
    }

    fn take(&self, index: u32, generation: u32) -> Option<Box<dyn Any>> {
        self.with_slot(index, generation, |slot| slot.handler.take())
    }

    /// Puts `handler` back only when the slot is still empty, so a `set` during `emit` wins.
    fn restore(&self, index: u32, generation: u32, handler: Option<Box<dyn Any>>) {
        self.with_slot(index, generation, |slot| {
            if slot.handler.is_none() {
                slot.handler = handler;
            }
        });
    }
}

// #region agent log
#[cfg(test)]
pub(crate) fn debug_listener_occupancy() -> (usize, usize) {
    RUNTIME.with(|runtime| {
        let slots = runtime.slots.borrow();
        let with_handler = slots
            .iter()
            .flatten()
            .filter(|slot| slot.handler.is_some())
            .count();
        (slots.len(), with_handler)
    })
}
// #endregion

/// A `Copy` handle for one component event.
///
/// `T` is the payload type. The closure is stored beside the handle, not inside it. [`Listener::emit`]
/// takes the closure out for the call, so a nested `emit` on the same listener does not recurse.
#[derive(PartialEq, Eq)]
pub struct Listener<T: 'static> {
    index: u32,
    generation: u32,
    _payload: PhantomData<fn(&T)>,
    _not_send: PhantomData<*const ()>,
}

impl<T: 'static> Clone for Listener<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Listener<T> {}

impl<T: 'static> fmt::Debug for Listener<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Listener")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T: 'static> Listener<T> {
    /// An empty slot. [`Listener::emit`] does nothing until [`Listener::set`] stores a closure.
    #[must_use]
    pub fn empty() -> Self {
        let (index, generation) = RUNTIME.with(Runtime::create);
        Self {
            index,
            generation,
            _payload: PhantomData,
            _not_send: PhantomData,
        }
    }

    /// Replaces the closure. The next [`Listener::emit`] calls `handler` and drops the previous one.
    pub fn set(&self, handler: impl FnMut(&T) + 'static) {
        let handler: Box<dyn FnMut(&T)> = Box::new(handler);
        RUNTIME.with(|runtime| runtime.store(self.index, self.generation, Box::new(handler)));
    }

    /// Calls the closure with `value`. Does nothing when no closure is stored.
    pub fn emit(&self, value: T) {
        let mut handler = RUNTIME.with(|runtime| runtime.take(self.index, self.generation));
        if let Some(erased) = handler.as_mut() {
            let callback = erased
                .downcast_mut::<Box<dyn FnMut(&T)>>()
                .expect("listener payload type does not match the handler");
            callback(&value);
        }
        RUNTIME.with(|runtime| runtime.restore(self.index, self.generation, handler));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::Listener;

    #[test]
    fn emit_without_a_handler_does_nothing() {
        Listener::<i32>::empty().emit(1);
    }

    #[test]
    fn emit_calls_the_handler_and_set_replaces_it() {
        let listener = Listener::<i32>::empty();
        let seen = Rc::new(Cell::new(0));
        let first = Rc::clone(&seen);
        listener.set(move |value| first.set(*value));
        listener.emit(1);
        assert_eq!(seen.get(), 1);

        let second = Rc::clone(&seen);
        listener.set(move |value| second.set(*value + 10));
        listener.emit(1);
        assert_eq!(seen.get(), 11);
    }

    #[test]
    fn set_during_emit_is_kept() {
        let listener = Listener::<()>::empty();
        let seen = Rc::new(Cell::new(0));
        let flag = Rc::clone(&seen);
        listener.set(move |_| {
            flag.set(flag.get() + 1);
            let flag = Rc::clone(&flag);
            listener.set(move |_| flag.set(flag.get() + 10));
        });
        listener.emit(());
        assert_eq!(seen.get(), 1);
        listener.emit(());
        assert_eq!(seen.get(), 11);
    }
}
