//! Hooks a component registers while it renders. Mount, unmount and frame ticks run them later.

use std::rc::Rc;

use crate::reactive::{Runtime, current_runtime, enter_runtime};

pub(crate) type Hook = Box<dyn FnMut()>;

/// Hooks collected for one component render, plus a lifecycle already stored on the same element
/// when this component returned that child directly.
#[derive(Default)]
pub(crate) struct Lifecycle {
    pub(crate) before_mount: Vec<Hook>,
    pub(crate) mounted: Vec<Hook>,
    pub(crate) before_unmount: Vec<Hook>,
    pub(crate) unmounted: Vec<Hook>,
    pub(crate) updated: Vec<Hook>,
    pub(crate) inner: Option<Box<Lifecycle>>,
}

#[derive(Default)]
pub(crate) struct HookFrame {
    before_mount: Vec<Hook>,
    mounted: Vec<Hook>,
    before_unmount: Vec<Hook>,
    unmounted: Vec<Hook>,
    updated: Vec<Hook>,
}

impl HookFrame {
    fn is_empty(&self) -> bool {
        self.before_mount.is_empty()
            && self.mounted.is_empty()
            && self.before_unmount.is_empty()
            && self.unmounted.is_empty()
            && self.updated.is_empty()
    }

    fn into_lifecycle(self) -> Lifecycle {
        Lifecycle {
            before_mount: self.before_mount,
            mounted: self.mounted,
            before_unmount: self.before_unmount,
            unmounted: self.unmounted,
            updated: self.updated,
            inner: None,
        }
    }
}

/// Pops the frame [`Runtime::begin_hooks`] pushed, including when `render` panics.
pub(crate) struct HookGuard {
    runtime: Rc<Runtime>,
    finished: bool,
}

impl HookGuard {
    /// The hooks registered in this frame. `None` when `render` registered nothing.
    pub(crate) fn finish(mut self) -> Option<Lifecycle> {
        self.finished = true;
        self.runtime.pop_hook_frame()
    }
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.runtime.pop_hook_frame();
        }
    }
}

impl Runtime {
    pub(crate) fn begin_hooks(self: &Rc<Self>) -> HookGuard {
        self.hook_frames.borrow_mut().push(HookFrame::default());
        HookGuard {
            runtime: Rc::clone(self),
            finished: false,
        }
    }

    fn pop_hook_frame(&self) -> Option<Lifecycle> {
        let frame = self.hook_frames.borrow_mut().pop()?;
        if frame.is_empty() {
            None
        } else {
            Some(frame.into_lifecycle())
        }
    }
}

/// Runs `hooks` while `runtime` is [`crate::Context::current`].
pub(crate) fn run_hooks(runtime: &Rc<Runtime>, hooks: &mut [Hook]) {
    if hooks.is_empty() {
        return;
    }
    enter_runtime(runtime, || {
        for hook in hooks {
            hook();
        }
    });
}

/// Registers `hook` to run before this component's nodes are mounted.
///
/// A parent hook runs before its children mount. Hooks on one component run in registration order.
///
/// # Panics
/// Panics when called outside [`Component::render`](crate::Component::render).
pub fn on_before_mount(hook: impl FnMut() + 'static) {
    register("on_before_mount", Box::new(hook), |frame| {
        &mut frame.before_mount
    });
}

/// Registers `hook` to run after this component's subtree is in the node tree.
///
/// Child `mounted` hooks run before the parent's. The first layout has not run yet, so a node's
/// rect is still empty. Hooks on one component run in registration order.
///
/// # Panics
/// Panics when called outside [`Component::render`](crate::Component::render).
pub fn on_mounted(hook: impl FnMut() + 'static) {
    register("on_mounted", Box::new(hook), |frame| &mut frame.mounted);
}

/// Registers `hook` to run before this component's nodes are disposed.
///
/// A parent hook runs before its children unmount. `show` hides a component and does not call
/// this. `if` and `for` do, when they drop the branch. Hooks run in registration order.
///
/// # Panics
/// Panics when called outside [`Component::render`](crate::Component::render).
pub fn on_before_unmount(hook: impl FnMut() + 'static) {
    register("on_before_unmount", Box::new(hook), |frame| {
        &mut frame.before_unmount
    });
}

/// Registers `hook` to run after this component's subtree has been disposed.
///
/// Child `unmounted` hooks run before the parent's. `show` does not call this. Hooks run in
/// registration order.
///
/// # Panics
/// Panics when called outside [`Component::render`](crate::Component::render).
pub fn on_unmounted(hook: impl FnMut() + 'static) {
    register("on_unmounted", Box::new(hook), |frame| &mut frame.unmounted);
}

/// Registers `hook` to run on every [`Ui::update`](crate::Ui::update) while this component is mounted.
///
/// Registering it keeps the host scheduling frames, the same way scroll and motion do. The mounted
/// tree stays. Writes the hook makes to signals flush in that same update, before layout. `show`
/// can hide the component and the hook still runs. Hooks run in registration order.
///
/// # Panics
/// Panics when called outside [`Component::render`](crate::Component::render).
pub fn on_updated(hook: impl FnMut() + 'static) {
    register("on_updated", Box::new(hook), |frame| &mut frame.updated);
}

fn register(name: &'static str, hook: Hook, slot: fn(&mut HookFrame) -> &mut Vec<Hook>) {
    let runtime = current_runtime();
    let mut frames = runtime.hook_frames.borrow_mut();
    let Some(frame) = frames.last_mut() else {
        panic!("{name} called outside a component render");
    };
    slot(frame).push(hook);
}
