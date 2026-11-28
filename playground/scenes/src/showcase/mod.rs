//! The showcase scene, split the way a cappy app usually is:
//!
//! - [`style`]: design tokens and the stylesheet. Tokens are the only thing that differs between
//!   themes; classes are applied to every theme;
//! - [`components`]: the shell. `App`, `Header` and `TabButton` each have their own file. The open
//!   tab is state on `App`. Each demo tab is its own component;
//! - [`store`]: singletons for one context. [`store::Counter`] is the shared count.
//!   [`store::AppStore`] is app-wide state other components read and write. Debug lives there;
//! - [`counter`]: the counter tab. It reads [`store::Counter`] and hosts one badge;
//! - [`badge`]: a component with its own state, a prop, an event, slots and a sheet.
//!   The counter tab hosts one. Badges are not nested;
//! - [`tiles`]: the tiles tab;
//! - [`checklist`]: the checklist tab;
//! - [`paint`]: the paint tab (linear gradients, drop shadows and a fitted image);
//! - [`images`]: the images tab (PNG, JPEG, GIF, WebP, BMP, ICO, and SVG decoded from files);
//! - [`motion`]: the motion tab.

mod badge;
mod checklist;
mod components;
mod counter;
mod images;
mod motion;
mod paint;
mod store;
mod style;
mod tiles;

use cappy::prelude::*;

/// Header with a theme switcher, and one tab per demo: counter, tiles, checklist, paint, motion.
pub fn showcase() -> Ui {
    // A runtime reused for another scene still holds the previous singletons.
    use_store(store::Counter).reset();
    use_store(store::AppStore).debug.set(false);
    Context::current().set_themes(style::themes().into_iter().map(style::stylesheet));
    // The catalog is already set, so this theme value is not installed.
    Ui::new(components::App::open(), Theme::light())
}
