//! Several window hosts in one process. The window manager stays outside this type: it maps its
//! own window id onto [`SurfaceId`] and stores the platform window next to the host.

use cappy_core::SurfaceId;

/// The primary window and the popup surfaces opened from it.
///
/// Closing the primary drops every popup. Closing a popup leaves the primary in place.
pub struct WindowSession<H> {
    primary: Option<H>,
    popups: Vec<(SurfaceId, H)>,
}

impl<H> WindowSession<H> {
    pub fn new() -> Self {
        Self {
            primary: None,
            popups: Vec::new(),
        }
    }

    pub fn set_primary(&mut self, host: H) {
        self.primary = Some(host);
    }

    pub fn primary(&self) -> Option<&H> {
        self.primary.as_ref()
    }

    pub fn primary_mut(&mut self) -> Option<&mut H> {
        self.primary.as_mut()
    }

    pub fn insert_popup(&mut self, id: SurfaceId, host: H) {
        if let Some((_, existing)) = self.popups.iter_mut().find(|(open, _)| *open == id) {
            *existing = host;
            return;
        }
        self.popups.push((id, host));
    }

    pub fn popup(&self, id: SurfaceId) -> Option<&H> {
        self.popups
            .iter()
            .find(|(open, _)| *open == id)
            .map(|(_, host)| host)
    }

    pub fn popup_mut(&mut self, id: SurfaceId) -> Option<&mut H> {
        self.popups
            .iter_mut()
            .find(|(open, _)| *open == id)
            .map(|(_, host)| host)
    }

    pub fn remove_popup(&mut self, id: SurfaceId) -> Option<H> {
        let index = self.popups.iter().position(|(open, _)| *open == id)?;
        Some(self.popups.remove(index).1)
    }

    /// Drops the primary and every popup. Returns whether a primary was open.
    pub fn close_primary(&mut self) -> bool {
        let had_primary = self.primary.take().is_some();
        self.popups.clear();
        had_primary
    }
}

impl<H> Default for WindowSession<H> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use cappy_core::{Application, SurfaceOptions, div};

    use super::*;

    fn two_ids() -> (SurfaceId, SurfaceId) {
        let context = Application::new().context().clone();
        let first = context.open_surface(SurfaceOptions::new("a"), |_| div());
        let second = context.open_surface(SurfaceOptions::new("b"), |_| div());
        let _ = context.drain_surfaces();
        (first, second)
    }

    #[test]
    fn closing_a_popup_keeps_the_primary() {
        let (first, second) = two_ids();
        let mut session = WindowSession::new();
        session.set_primary("primary");
        session.insert_popup(first, "one");
        session.insert_popup(second, "two");

        assert_eq!(session.remove_popup(first), Some("one"));
        assert_eq!(session.primary(), Some(&"primary"));
        assert_eq!(session.popup(second), Some(&"two"));
        assert_eq!(session.remove_popup(first), None);

        assert!(session.close_primary());
        assert_eq!(session.primary(), None);
        assert_eq!(session.popup(second), None);
        assert!(!session.close_primary());
    }
}
