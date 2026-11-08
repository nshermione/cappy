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

/// One value of this type per [`Context`]. `#[store]` on a struct implements it.
pub trait Singleton: Clone + 'static {
    /// Builds the value stored the first time [`Context::use_store`] sees this type.
    fn create(cx: &Context) -> Self;
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

/// Builds one field of a `#[store]` struct.
///
/// [`Default`](Default) types use their default. [`crate::Reactive`] builds a new handle from the
/// inner default. The `#[store]` macro implements this for the struct itself.
#[doc(hidden)]
pub trait FieldInit: Sized {
    fn field_init(cx: &Context) -> Self;
}

impl<T: Default> FieldInit for T {
    fn field_init(_cx: &Context) -> Self {
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
        self.runtime.push_component(component);
        let _pop = PopComponent(Rc::clone(&self.runtime));
        body()
    }

    /// Binds `method` to the component [`Context::with`] is rendering.
    ///
    /// The returned closure owns a clone of that component and of this context, so it outlives
    /// `render`. The method receives `&self`, `&Context`, then the event.
    ///
    /// # Panics
    /// Panics when called outside [`Context::with`], or when `method`'s receiver is not the
    /// component currently on the stack.
    pub fn on<T, E>(&self, method: fn(&T, &Context, &E)) -> impl FnMut(&E) + use<T, E>
    where
        T: 'static,
        E: 'static,
    {
        bind_handler(self.clone(), method)
    }

    /// Runs `body` while this context is [`Context::current`].
    ///
    /// Nested calls push their own runtime index and pop it before this one is current again.
    /// Entering the same context twice pushes its index twice.
    pub fn enter<R>(&self, body: impl FnOnce() -> R) -> R {
        crate::reactive::enter_runtime(&self.runtime, body)
    }

    /// The context the innermost [`Context::enter`] selected.
    ///
    /// # Panics
    /// Panics when called outside [`Context::enter`].
    pub fn current() -> Self {
        Self {
            runtime: crate::reactive::current_runtime(),
        }
    }
}

/// [`Context::reactive`] on [`Context::current`].
///
/// # Panics
/// Panics when called outside [`Context::enter`].
pub fn reactive<T: crate::reactive::IntoReactive>(value: T) -> T::Output {
    Context::current().reactive(value)
}

/// [`Context::use_store`] on [`Context::current`].
///
/// # Panics
/// Panics when called outside [`Context::enter`].
pub fn use_store<T: Singleton>(of: SingletonOf<T>) -> T {
    Context::current().use_store(of)
}

/// [`Context::use_local_store`] on [`Context::current`].
///
/// # Panics
/// Panics when called outside [`Context::enter`].
pub fn use_local_store<T: Singleton>(of: SingletonOf<T>) -> T {
    Context::current().use_local_store(of)
}

/// [`Context::on`] on [`Context::current`].
///
/// # Panics
/// Panics when called outside [`Context::enter`], or when [`Context::on`] would panic.
pub fn on<T, E>(method: fn(&T, &Context, &E)) -> impl FnMut(&E) + use<T, E>
where
    T: 'static,
    E: 'static,
{
    bind_handler(Context::current(), method)
}

fn bind_handler<T, E>(cx: Context, method: fn(&T, &Context, &E)) -> impl FnMut(&E) + use<T, E>
where
    T: 'static,
    E: 'static,
{
    let frame = cx
        .runtime
        .components
        .borrow()
        .last()
        .cloned()
        .expect("cx.on called outside cx.with");
    let instance = frame
        .downcast::<T>()
        .unwrap_or_else(|_| panic!("cx.on method does not belong to the component being rendered"));
    move |event| method(&instance, &cx, event)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::{Application, Context};

    #[test]
    fn nested_enter_keeps_each_runtime() {
        let first = Application::new();
        let second = Application::new();
        first.context().enter(|| {
            let left = Context::current().reactive(1_i32);
            assert!(Rc::ptr_eq(
                &Context::current().runtime,
                &first.context().runtime
            ));
            let right = second.context().enter(|| {
                assert!(Rc::ptr_eq(
                    &Context::current().runtime,
                    &second.context().runtime
                ));
                Context::current().reactive(2_i32)
            });
            assert!(Rc::ptr_eq(
                &Context::current().runtime,
                &first.context().runtime
            ));
            left.set(3);
            assert_eq!(left.get(), 3);
            assert_eq!(right.get(), 2);
        });
    }
}

struct PopComponent(Rc<Runtime>);

impl Drop for PopComponent {
    fn drop(&mut self) {
        self.0.pop_component();
    }
}
