//! The showcase scene, split the way a cappy app usually is:
//!
//! - [`theme`]: design tokens (colors, spacing, radius, font sizes), the only thing that differs
//!   between themes;
//! - [`style`]: the stylesheet, i.e. the classes the layout uses, applied to every theme;
//! - [`layout`]: the node tree, its state and its event handlers, with no styling of its own.

mod layout;
mod style;
mod theme;

use std::rc::Rc;
use std::sync::Arc;

use cappy::prelude::*;

/// Header with a theme switcher, a counter, selectable tiles and a task list.
pub fn showcase() -> Ui {
    let themes: Rc<[Arc<Theme>]> = theme::themes()
        .into_iter()
        .map(|tokens| Arc::new(style::stylesheet(tokens)))
        .collect();
    let active_theme = signal(Arc::clone(&themes[0]));
    Ui::with_theme_signal(layout::root(active_theme, themes), active_theme)
}
