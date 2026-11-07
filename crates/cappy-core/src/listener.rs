//! Clone handles for component events.
//!
//! The closure lives in the same runtime as signals. [`Listener`] stores the slot id and an
//! [`Rc`](std::rc::Rc) to that runtime, so a component that holds listeners stays [`Clone`].

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::Context;
use crate::reactive::{ListenerSlot, Runtime};

const DEAD_LISTENER: &str = "listener was disposed";

impl Runtime {
    pub(crate) fn create_listener(self: &Rc<Self>) -> (Rc<Self>, u32, u32) {
        let generation = {
            let next = &self.next_listener_generation;
            let generation = next.get().wrapping_add(1);
            next.set(generation);
            generation
        };
        let mut slots = self.listeners.borrow_mut();
        slots.push(Some(ListenerSlot {
            generation,
            handler: None,
        }));
        let index = u32::try_from(slots.len() - 1).expect("too many live listeners");
        (Rc::clone(self), index, generation)
    }

    fn with_listener<R>(
        &self,
        index: u32,
        generation: u32,
        body: impl FnOnce(&mut ListenerSlot) -> R,
    ) -> R {
        let mut slots = self.listeners.borrow_mut();
        let slot = slots
            .get_mut(index as usize)
            .and_then(Option::as_mut)
            .filter(|slot| slot.generation == generation)
            .expect(DEAD_LISTENER);
        body(slot)
    }
}

/// A [`Clone`] handle for one component event.
///
/// `T` is the payload type. The closure is stored beside the handle, not inside it.
/// [`Listener::emit`] takes the closure out for the call, so a nested `emit` on the same listener
/// does not recurse.
pub struct Listener<T: 'static> {
    runtime: Rc<Runtime>,
    index: u32,
    generation: u32,
    _payload: PhantomData<fn(&T)>,
}

impl<T: 'static> Clone for Listener<T> {
    fn clone(&self) -> Self {
        Self {
            runtime: Rc::clone(&self.runtime),
            index: self.index,
            generation: self.generation,
            _payload: PhantomData,
        }
    }
}

impl<T: 'static> PartialEq for Listener<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.runtime, &other.runtime)
            && self.index == other.index
            && self.generation == other.generation
    }
}

impl<T: 'static> Eq for Listener<T> {}

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
    pub fn empty(context: &Context) -> Self {
        let (runtime, index, generation) = context.runtime.create_listener();
        Self {
            runtime,
            index,
            generation,
            _payload: PhantomData,
        }
    }

    /// Replaces the closure. The next [`Listener::emit`] calls `handler` and drops the previous one.
    pub fn set(&self, handler: impl FnMut(&T) + 'static) {
        let handler: Box<dyn FnMut(&T)> = Box::new(handler);
        self.runtime
            .with_listener(self.index, self.generation, |slot| {
                slot.handler = Some(Box::new(handler));
            });
    }

    /// Calls the closure with `value`. Does nothing when no closure is stored.
    pub fn emit(&self, value: T) {
        let mut handler = self
            .runtime
            .with_listener(self.index, self.generation, |slot| slot.handler.take());
        if let Some(erased) = handler.as_mut() {
            let callback = erased
                .downcast_mut::<Box<dyn FnMut(&T)>>()
                .expect("listener payload type does not match the handler");
            callback(&value);
        }
        self.runtime
            .with_listener(self.index, self.generation, |slot| {
                if slot.handler.is_none() {
                    slot.handler = handler;
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::Listener;
    use crate::Application;

    fn context() -> crate::Context {
        Application::new().context().clone()
    }

    #[test]
    fn emit_without_a_handler_does_nothing() {
        Listener::<i32>::empty(&context()).emit(1);
    }

    #[test]
    fn emit_calls_the_handler_and_set_replaces_it() {
        let listener = Listener::<i32>::empty(&context());
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
        let listener = Listener::<()>::empty(&context());
        let during = listener.clone();
        let seen = Rc::new(Cell::new(0));
        let flag = Rc::clone(&seen);
        listener.set(move |_| {
            flag.set(flag.get() + 1);
            let flag = Rc::clone(&flag);
            during.set(move |_| flag.set(flag.get() + 10));
        });
        listener.emit(());
        assert_eq!(seen.get(), 1);
        listener.emit(());
        assert_eq!(seen.get(), 11);
    }
}
