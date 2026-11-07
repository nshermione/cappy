//! The showcase scene, split the way a cappy app usually is:
//!
//! - [`theme`]: design tokens (colors, spacing, radius, font sizes), the only thing that differs
//!   between themes;
//! - [`style`]: the stylesheet, i.e. the classes the layout uses, applied to every theme;
//! - [`layout`]: the node tree, its state and its event handlers, with no styling of its own;
//! - [`counter`]: a composable with one singleton per context and separate `new` instances;
//! - [`badge`]: a single-file component with its own state, props, event, slots and sheet.
//!   Its demo nests one badge inside another's slot.

mod badge;
mod counter;
mod layout;
mod style;
mod theme;

use std::rc::Rc;
use std::sync::Arc;

use cappy::prelude::*;

/// Header with a theme switcher, and one tab per demo: counter, tiles, checklist, nested badge.
pub fn showcase(cx: &Context) -> Ui {
    // A context reused for another scene still holds the previous singleton.
    cx.use_store(counter::Counter).reset();
    let themes: Rc<[Arc<Theme>]> = theme::themes()
        .into_iter()
        .map(|tokens| Arc::new(style::stylesheet(tokens)))
        .collect();
    let active_theme = cx.reactive(Arc::clone(&themes[0]));
    Ui::with_theme_signal(
        cx,
        layout::root(cx, active_theme.clone(), themes),
        active_theme,
    )
}
