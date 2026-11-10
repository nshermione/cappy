//! The showcase scene, split the way a cappy app usually is:
//!
//! - [`style`]: design tokens and the stylesheet. Tokens are the only thing that differs between
//!   themes; classes are applied to every theme;
//! - [`layout`]: the shell (header and tab bar). Each tab is its own component;
//! - [`store`]: singletons for one context. [`store::Counter`] is the shared count.
//!   [`store::App`] is the open tab;
//! - [`counter`]: the counter tab. It reads [`store::Counter`] and hosts one badge;
//! - [`badge`]: a component with its own state, a prop, an event, slots and a sheet.
//!   The counter tab hosts one. Badges are not nested;
//! - [`tiles`]: the tiles tab;
//! - [`checklist`]: the checklist tab;
//! - [`paint`]: the paint tab (linear gradients, drop shadows and a fitted image);
//! - [`images`]: the images tab (PNG, JPEG, GIF, WebP, BMP, ICO, and SVG decoded from files);
//! - motion: the motion tab, when the `cpu` feature is off. That feature does not export motion
//!   from cappy, so this module is not compiled and a call to `animate` does not build.

mod badge;
mod checklist;
mod counter;
mod images;
mod layout;
#[cfg(not(feature = "cpu"))]
mod motion;
mod paint;
mod store;
#[cfg_attr(feature = "cpu", allow(dead_code))]
mod style;
mod tiles;

use cappy::prelude::*;

/// Header with a theme switcher, and one tab per demo: counter, tiles, checklist, paint, motion.
pub fn showcase(cx: &Context) -> Ui {
    cx.enter(|| {
        // A context reused for another scene still holds the previous singletons.
        use_store(store::Counter).reset();
        use_store(store::App).select(store::Tab::Counter);
        cx.set_themes(style::themes().into_iter().map(style::stylesheet));
        // The catalog is already set, so this theme value is not installed.
        Ui::new(cx, layout::root(), Theme::light())
    })
}
