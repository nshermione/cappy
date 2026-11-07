//! The tiles tab. One selected index, shared by every tile.

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::style::{Class, class_when};

const TILE_COUNT: usize = 6;

#[component]
pub struct TilesTab {
    selected: Option<usize>,
}

impl Component for TilesTab {
    fn render(&self, _cx: &Context) -> Element {
        let selected = self.selected;
        let tiles = (0..TILE_COUNT).map(|index| tile(index, selected));
        ui! {
            div(class = [ThemeClass::Card, Class::Grow]) {
                text("Tiles", class = ThemeClass::Subtitle)
                div(class = Class::TileGrid) {
                    { ..tiles }
                }
                text(:match self.selected.get() {
                    Some(index) => format!("Selected tile {}", index + 1),
                    None => "Click a tile".to_owned(),
                }, class = ThemeClass::Muted, key = "selection")
            }
        }
    }
}

fn tile(index: usize, selected: Reactive<Option<usize>>) -> Element {
    ui! {
        div(
            class = ThemeClass::Tile,
            :class = class_when(selected.get() == Some(index), Class::TileSelected),
            key = format!("tile-{index}"),
            on_click = move |_| selected.set(Some(index)),
        ) {
            text(format!("Tile {}", index + 1))
        }
    }
}
