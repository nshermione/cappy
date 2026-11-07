//! The value passed through the UI. [`Application`] owns one [`Context`]. Signals, listeners and
//! component render all use that same runtime.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::reactive::Runtime;

/// Root of a cappy process. Window code receives this. UI code receives [`Context`].
pub struct Application {
    context: Context,
}

impl Application {
    pub fn new() -> Self {
        Self {
            context: Context {
                runtime: Rc::new(Runtime::default()),
            },
        }
    }

    /// The handle UI code threads from this application downward.
    pub fn context(&self) -> &Context {
        &self.context
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

/// One value of this type per [`Context`]. `#[reactive(singleton)]` implements it.
pub trait Singleton: Clone + 'static {
    /// Builds the value stored the first time [`Context::singleton`] sees this type.
    fn create(cx: &Context) -> Self;
}

/// Marker passed to [`Context::singleton`]. `#[reactive(singleton)]` emits one under the struct's name.
pub struct SingletonOf<T> {
    _marker: PhantomData<fn() -> T>,
}

impl<T> SingletonOf<T> {
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for SingletonOf<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SingletonOf<T> {}

/// Handle to one reactive runtime. Cloning clones an [`Rc`](std::rc::Rc).
#[derive(Clone)]
pub struct Context {
    pub(crate) runtime: Rc<Runtime>,
}

impl fmt::Debug for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Context")
    }
}

impl Context {
    /// Runs `body` while `component` is the one [`Context::on`] binds.
    ///
    /// Nested calls push their own component and pop it before this one is visible again.
    pub fn with<T, R>(&self, component: &T, body: impl FnOnce() -> R) -> R
    where
        T: Clone + 'static,
    {
        self.runtime.push_component(component);
        let _pop = PopComponent(Rc::clone(&self.runtime));
        body()
    }

    /// Binds `method` to the component [`Context::with`] is rendering.
    ///
    /// The returned closure owns a clone of that component, so it outlives `render`.
    ///
    /// # Panics
    /// Panics when called outside [`Context::with`], or when `method`'s receiver is not the
    /// component currently on the stack.
    pub fn on<T, E>(&self, method: fn(&T, &E)) -> impl FnMut(&E) + 'static
    where
        T: 'static,
        E: 'static,
    {
        let frame = self
            .runtime
            .components
            .borrow()
            .last()
            .cloned()
            .expect("cx.on called outside cx.with");
        let instance = frame.downcast::<T>().unwrap_or_else(|_| {
            panic!("cx.on method does not belong to the component being rendered")
        });
        move |event| method(&instance, event)
    }
}

struct PopComponent(Rc<Runtime>);

impl Drop for PopComponent {
    fn drop(&mut self) {
        self.0.pop_component();
    }
}
