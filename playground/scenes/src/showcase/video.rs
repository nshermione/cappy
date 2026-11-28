//! The video tab. File and HLS are the same player; the element owns the controls.

use std::path::PathBuf;

use cappy::Class as ThemeClass;
use cappy::prelude::*;

use super::style::Class;

const MUX_BUNNY: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";

#[component]
pub struct VideoTab {}

impl Component for VideoTab {
    fn render(&self) -> Element {
        let hls = reactive(false);
        let file_mark = hls.clone();
        let hls_mark = hls.clone();
        let choose_file = hls.clone();
        let choose_hls = hls.clone();
        let choice = hls.clone();
        element! {
            div(class = [ThemeClass::Card, Class::Grow]) {
                text("Video", class = ThemeClass::Subtitle)
                text(
                    "Big Buck Bunny. File is a 12 second local excerpt. HLS is the full Mux stream. Hover the picture for controls.",
                    class = ThemeClass::Muted,
                ),
                div(class = ThemeClass::Row) {
                    div(
                        class = ThemeClass::Btn,
                        :class = if file_mark.get() { "" } else { "btn-primary" },
                        key = "video-source-file",
                        on_click = move |_| choose_file.set(false),
                    ) {
                        text("File")
                    }
                    div(
                        class = ThemeClass::Btn,
                        :class = if hls_mark.get() { "btn-primary" } else { "" },
                        key = "video-source-hls",
                        on_click = move |_| choose_hls.set(true),
                    ) {
                        text("HLS")
                    }
                }
                { if_else(
                    move || choice.get(),
                    || cappy_video::player(MUX_BUNNY).into_element(),
                    || cappy_video::player(bunny_path().display().to_string()).into_element(),
                ) }
            }
        }
    }
}

fn bunny_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/big_buck_bunny.mp4")
}
