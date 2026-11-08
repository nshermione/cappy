//! The showcase scene, split the way a cappy app usually is:
//!
//! - [`theme`]: design tokens (colors, spacing, radius, font sizes), the only thing that differs
//!   between themes;
//! - [`style`]: the stylesheet, i.e. the classes the layout uses, applied to every theme;
//! - [`layout`]: the shell (header and tab bar). Each tab is its own component;
//! - [`store`]: singletons for one context. [`store::Counter`] is the shared count.
//!   [`store::App`] is the open tab;
//! - [`counter`]: the counter tab. It reads [`store::Counter`] and hosts one badge;
//! - [`badge`]: a component with its own state, a prop, an event, slots and a sheet.
//!   The counter tab hosts one. Badges are not nested;
//! - [`tiles`]: the tiles tab;
//! - [`checklist`]: the checklist tab;
//! - [`paint`]: the paint tab (linear gradients, drop shadows and a fitted image).

mod badge;
mod checklist;
mod counter;
mod layout;
mod paint;
mod store;
mod style;
mod theme;
mod tiles;

use cappy::prelude::*;

/// Header with a theme switcher, and one tab per demo: counter, tiles, checklist, paint.
pub fn showcase(cx: &Context) -> Ui {
    // A context reused for another scene still holds the previous singletons.
    cx.use_store(store::Counter).reset();
    cx.use_store(store::App).select(store::Tab::Counter);
    cx.set_themes(theme::themes().into_iter().map(style::stylesheet));
    // The catalog is already set, so this theme value is not installed.
    Ui::new(cx, layout::root(cx), Theme::light())
}
