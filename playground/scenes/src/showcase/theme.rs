//! Design tokens. A theme decides how the scene looks (colors, radius, spacing) but never where
//! nodes go, so switching themes only repaints.

use cappy::prelude::*;

/// The themes the header button cycles through, in order.
pub fn themes() -> [Theme; 3] {
    [Theme::light(), Theme::dark(), ocean()]
}

/// A custom theme: the dark one with blue surfaces and rounder corners.
fn ocean() -> Theme {
    theme! {
        name: "ocean",
        extends: Theme::dark(),
        colors {
            background: "#0b1d2a",
            surface: "#10293b",
            surface_alt: "#15354c",
            surface_hover: "#1c4462",
            border: "#24506f",
            primary: "#0ea5e9",
            primary_hover: "#38bdf8",
            primary_active: "#0284c7",
            on_primary: "#04121c",
        },
        radius { md: 10.0, lg: 16.0 },
    }
}
