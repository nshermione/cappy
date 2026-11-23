//! The value passed through the UI. [`Application::new`] installs one runtime on this thread.
//! Signals, listeners and component render all use it. [`Context::current`] reads that runtime.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::reactive::{Runtime, runtime};

/// Installs this thread's runtime. A later call replaces it. Signals and [`crate::Ui`] values
/// from the previous runtime must not be used after that.
pub struct Application;

impl Application {
    pub fn new() -> Self {
        crate::reactive::install_runtime();
        Self
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

/// One value of this type per runtime. `#[store]` on a struct implements it.
pub trait Singleton: Clone + 'static {
    /// Builds the value stored the first time [`Context::use_store`] sees this type.
    fn create() -> Self;
}

/// Marker passed to [`Context::use_store`]. `#[store]` emits one under the struct's name.
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

/// Handle to the thread's runtime. It holds no pointer: every method reads [`Context::current`]'s
/// runtime.
#[derive(Clone, Copy)]
pub struct Context;

impl fmt::Debug for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Context")
    }
}

/// Builds one field of a `#[store]` struct.
///
/// [`Default`](Default) types use their default. [`crate::Reactive`] builds a new handle from the
/// inner default. The `#[store]` macro implements this for the struct itself.
#[doc(hidden)]
pub trait FieldInit: Sized {
    fn field_init() -> Self;
}

impl<T: Default> FieldInit for T {
    fn field_init() -> Self {
        T::default()
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
        let runtime = runtime();
        runtime.push_component(component);
        let _pop = PopComponent(Rc::clone(&runtime));
        body()
    }

    /// Binds `method` to the component [`Context::with`] is rendering.
    ///
    /// The returned closure owns a clone of that component, so it outlives `render`. The method
    /// receives `&self`, then the event. The call runs inside [`Context::with`], so a nested
    /// [`Context::on`] sees that component.
    ///
    /// # Panics
    /// Panics when called outside [`Context::with`], or when `method`'s receiver is not the
    /// component currently on the stack.
    pub fn on<T, E>(&self, method: fn(&T, &E)) -> impl FnMut(&E) + use<T, E>
    where
        T: Clone + 'static,
        E: 'static,
    {
        let _ = self;
        bind_handler(method)
    }

    /// The runtime [`Application::new`] installed on this thread.
    ///
    /// # Panics
    /// Panics when [`Application::new`] has not run on this thread.
    pub fn current() -> Self {
        let _ = runtime();
        Self
    }
}

/// [`Context::reactive`] on [`Context::current`].
///
/// # Panics
/// Panics when [`Application::new`] has not run on this thread.
pub fn reactive<T: crate::reactive::IntoReactive>(value: T) -> T::Output {
    Context::current().reactive(value)
}

/// [`Context::use_store`] on [`Context::current`].
///
/// # Panics
/// Panics when [`Application::new`] has not run on this thread.
pub fn use_store<T: Singleton>(of: SingletonOf<T>) -> T {
    Context::current().use_store(of)
}

/// [`Context::use_local_store`] on [`Context::current`].
///
/// # Panics
/// Panics when [`Application::new`] has not run on this thread.
pub fn use_local_store<T: Singleton>(of: SingletonOf<T>) -> T {
    Context::current().use_local_store(of)
}

/// [`Context::on`] on [`Context::current`].
///
/// # Panics
/// Panics when [`Application::new`] has not run on this thread, or when [`Context::on`] would
/// panic.
pub fn on<T, E>(method: fn(&T, &E)) -> impl FnMut(&E) + use<T, E>
where
    T: Clone + 'static,
    E: 'static,
{
    let _ = Context::current();
    bind_handler(method)
}

fn bind_handler<T, E>(method: fn(&T, &E)) -> impl FnMut(&E) + use<T, E>
where
    T: Clone + 'static,
    E: 'static,
{
    let frame = runtime()
        .components
        .borrow()
        .last()
        .cloned()
        .expect("cx.on called outside cx.with");
    let instance = frame
        .downcast::<T>()
        .unwrap_or_else(|_| panic!("cx.on method does not belong to the component being rendered"));
    move |event| {
        Context.with(instance.as_ref(), || method(instance.as_ref(), event));
    }
}

struct PopComponent(Rc<Runtime>);

impl Drop for PopComponent {
    fn drop(&mut self) {
        self.0.pop_component();
    }
}
