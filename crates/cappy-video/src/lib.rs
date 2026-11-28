//! Decodes a local video file on a worker thread and publishes pictures to a [`VideoSource`].
//! Sound is decoded only when the opener asks for it.

mod dash;
mod decode;
mod file;
mod hls;
mod locator;
mod packet;
mod player;
mod playlist;
mod scrub;
mod segment;
mod session;
mod sound;
mod vtt;
mod worker;

#[cfg(test)]
mod harness;
#[cfg(test)]
mod pipeline;

pub use file::{OpenOptions, open, open_with, sample_path};
pub use player::{Player, player};
pub use playlist::{open_dash, open_hls};
pub use scrub::{Scrub, scrub_action, scrub_pts};
