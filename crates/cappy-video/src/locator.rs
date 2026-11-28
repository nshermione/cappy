//! Local paths and `http`/`https` locators. Playlists store either one in a path buffer.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

pub(crate) fn is_remote(locator: &str) -> bool {
    let lower = locator.as_bytes();
    starts_with_ignore_ascii_case(lower, b"https://")
        || starts_with_ignore_ascii_case(lower, b"http://")
}

pub(crate) fn read_text(locator: &Path) -> Result<String, String> {
    let bytes = read_bytes(locator)?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not utf-8", locator.display()))
}

pub(crate) fn read_bytes(locator: &Path) -> Result<Vec<u8>, String> {
    let text = locator.to_string_lossy();
    if is_remote(&text) {
        return begin_fetch(text.as_ref()).wait();
    }
    std::fs::read(locator).map_err(|error| error.to_string())
}

/// Starts the download if it is not already running. Playback can keep decoding
/// the current segment while this one arrives.
pub(crate) fn prefetch(locator: &Path) {
    let text = locator.to_string_lossy();
    if is_remote(&text) {
        let _ = begin_fetch(text.as_ref());
    }
}

enum FetchState {
    Pending,
    Ready(Result<Vec<u8>, String>),
    Spent,
}

struct Flight {
    state: Mutex<FetchState>,
    done: Condvar,
}

struct PendingFetch {
    locator: String,
    flight: Arc<Flight>,
}

fn flights() -> &'static Mutex<Option<PendingFetch>> {
    static FLIGHTS: OnceLock<Mutex<Option<PendingFetch>>> = OnceLock::new();
    FLIGHTS.get_or_init(|| Mutex::new(None))
}

fn begin_fetch(locator: &str) -> Arc<Flight> {
    let mut slot = flights().lock().expect("segment");
    if let Some(pending) = slot.as_ref()
        && pending.locator == locator
        && !pending.flight.is_spent()
    {
        return Arc::clone(&pending.flight);
    }
    let flight = Arc::new(Flight {
        state: Mutex::new(FetchState::Pending),
        done: Condvar::new(),
    });
    let writer = Arc::clone(&flight);
    let owned = locator.to_owned();
    std::thread::spawn(move || writer.finish(fetch_remote(&owned)));
    *slot = Some(PendingFetch {
        locator: locator.to_owned(),
        flight: Arc::clone(&flight),
    });
    flight
}

impl Flight {
    fn is_spent(&self) -> bool {
        matches!(*self.state.lock().expect("segment"), FetchState::Spent)
    }

    fn finish(&self, result: Result<Vec<u8>, String>) {
        let mut state = self.state.lock().expect("segment");
        *state = FetchState::Ready(result);
        self.done.notify_all();
    }

    fn wait(&self) -> Result<Vec<u8>, String> {
        let mut state = self.state.lock().expect("segment");
        loop {
            match std::mem::replace(&mut *state, FetchState::Pending) {
                FetchState::Pending => {
                    state = self.done.wait(state).expect("segment");
                }
                FetchState::Ready(result) => {
                    *state = FetchState::Spent;
                    return result;
                }
                FetchState::Spent => {
                    *state = FetchState::Spent;
                    return Err("segment was already read".to_owned());
                }
            }
        }
    }
}

fn fetch_remote(locator: &str) -> Result<Vec<u8>, String> {
    let response = agent()
        .get(locator)
        .call()
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// Resolves `reference` against a playlist path or an absolute URL.
pub(crate) fn resolve(base: &Path, reference: &str) -> PathBuf {
    if is_remote(reference) {
        return PathBuf::from(reference);
    }
    let base_text = base.to_string_lossy();
    if is_remote(&base_text) {
        return PathBuf::from(join_url(&base_text, reference));
    }
    let reference_path = Path::new(reference);
    if reference_path.is_absolute() {
        return reference_path.to_path_buf();
    }
    base.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(reference_path)
}

fn join_url(base: &str, reference: &str) -> String {
    let (origin, dir) = split_dir(base);
    if let Some(absolute) = reference.strip_prefix('/') {
        return if absolute.is_empty() {
            format!("{origin}/")
        } else {
            format!("{origin}/{absolute}")
        };
    }
    let mut stack: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    if stack.is_empty() {
        format!("{origin}/")
    } else {
        format!("{origin}/{}", stack.join("/"))
    }
}

fn split_dir(url: &str) -> (String, String) {
    let scheme = url.find("://").map(|index| index + 3).unwrap_or(0);
    let Some(relative) = url[scheme..].find('/') else {
        return (url.to_owned(), String::new());
    };
    let path_start = scheme + relative;
    let origin = url[..path_start].to_owned();
    let path = &url[path_start + 1..];
    let dir = match path.rfind('/') {
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    };
    (origin, dir)
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(45))
            .build()
    })
}

fn starts_with_ignore_ascii_case(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_segment_stays_beside_its_playlist() {
        let path = resolve(
            Path::new("https://test-streams.mux.dev/x36xhzz/url_0/media.m3u8"),
            "url_462/segment.ts",
        );
        assert_eq!(
            path,
            Path::new("https://test-streams.mux.dev/x36xhzz/url_0/url_462/segment.ts")
        );
    }

    #[test]
    fn an_absolute_url_is_kept() {
        let path = resolve(
            Path::new("https://example.com/a/master.m3u8"),
            "https://cdn.example/seg.ts",
        );
        assert_eq!(path, Path::new("https://cdn.example/seg.ts"));
    }

    #[test]
    fn a_local_segment_joins_the_playlist_directory() {
        let path = resolve(Path::new("fixtures/hls/vod.m3u8"), "seg0.ts");
        assert_eq!(path, Path::new("fixtures/hls/seg0.ts"));
    }
}
