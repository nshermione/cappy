//! The images tab. Each picture is loaded from this crate's `assets` directory.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::style::Class;

pub fn images_tab() -> Element {
    element! {
        div(class = [ThemeClass::Card, Class::Grow]) {
            text("Images", class = ThemeClass::Subtitle)
            text(
                "Loaded from a file path on desktop and a URL in the browser. GIF plays.",
                class = ThemeClass::Muted,
            )
            div(class = ThemeClass::Row) {
                { specimen("PNG", "image-png", "sample.png") }
                { specimen("JPEG", "image-jpeg", "sample.jpg") }
                { specimen("GIF", "image-gif", "sample.gif") }
                { specimen("WebP", "image-webp", "sample.webp") }
            }
            div(class = ThemeClass::Row) {
                { specimen("BMP", "image-bmp", "sample.bmp") }
                { specimen("ICO", "image-ico", "sample.ico") }
                { specimen("SVG", "image-svg", "sample.svg") }
            }
        }
    }
}

fn specimen(caption: &'static str, key: &'static str, file: &str) -> Element {
    let locator = asset_path!(format!("assets/{file}"));
    element! {
        div(class = Class::Specimen) {
            image(locator, fit = ObjectFit::Contain, class = Class::Picture, key = key)
            text(caption, class = ThemeClass::Muted)
        }
    }
}
