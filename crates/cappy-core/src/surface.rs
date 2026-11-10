//! Requests for another OS window. The context queues them. A host drains the queue and creates
//! the window. This module does not know which window manager that host uses.

use crate::Context;
use crate::element::Element;

/// Identifies one popup surface requested from a [`Context`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(u64);

/// How a host should open a popup window.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceOptions {
    pub title: String,
    pub logical_size: (f64, f64),
    /// Logical offset from the owner window's outer top-left corner.
    pub offset: (f64, f64),
}

impl SurfaceOptions {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            logical_size: (360.0, 240.0),
            offset: (32.0, 32.0),
        }
    }

    #[must_use]
    pub fn with_size(mut self, width: f64, height: f64) -> Self {
        self.logical_size = (width, height);
        self
    }

    #[must_use]
    pub fn with_offset(mut self, x: f64, y: f64) -> Self {
        self.offset = (x, y);
        self
    }
}

/// One open or close, in the order [`Context::open_surface`] and [`Context::close_surface`] ran.
#[derive(Debug)]
pub enum SurfaceCommand {
    Open {
        id: SurfaceId,
        options: SurfaceOptions,
        root: Element,
    },
    Close(SurfaceId),
}

impl Context {
    /// Queues a popup window and wakes the host.
    ///
    /// `build` receives the id so the popup can close itself. The element is mounted later, on
    /// the new [`crate::Ui`]. Calling this again opens another window.
    pub fn open_surface(
        &self,
        options: SurfaceOptions,
        build: impl FnOnce(SurfaceId) -> Element,
    ) -> SurfaceId {
        let id = self.allocate_surface();
        let root = build(id);
        self.runtime
            .surfaces
            .borrow_mut()
            .push(SurfaceCommand::Open { id, options, root });
        self.runtime.wake_host();
        id
    }

    /// Queues a close for a popup opened with [`Context::open_surface`].
    ///
    /// Closing an id the host has already dropped does nothing.
    pub fn close_surface(&self, id: SurfaceId) {
        self.runtime
            .surfaces
            .borrow_mut()
            .push(SurfaceCommand::Close(id));
        self.runtime.wake_host();
    }

    /// Takes every queued open and close. The next call is empty until another request arrives.
    pub fn drain_surfaces(&self) -> Vec<SurfaceCommand> {
        std::mem::take(&mut *self.runtime.surfaces.borrow_mut())
    }

    fn allocate_surface(&self) -> SurfaceId {
        let id = self.runtime.next_surface.get().wrapping_add(1);
        self.runtime.next_surface.set(id);
        SurfaceId(id)
    }
}
