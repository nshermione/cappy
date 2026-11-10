//! Load an image locator into an [`ImageClip`](crate::ImageClip).
//!
//! A filesystem path is read on the UI thread. `http`/`https`, and every locator in the browser,
//! finish later. The node keeps a slot and the host keeps waking until the bytes land.

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::bitmap::ImageClip;
use crate::image_source::ImageSource;

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

/// A load that is still running. [`take`](Self::take) is `Some` once, when it finishes.
pub(crate) struct ImageLoad {
    slot: Arc<Mutex<Option<Result<ImageClip, String>>>>,
}

impl ImageLoad {
    pub(crate) fn take(&self) -> Option<Result<ImageClip, String>> {
        self.slot.lock().ok()?.take()
    }
}

/// Turns `source` into a clip. A locator that cannot be read yet comes back with a load slot
/// and a transparent stand-in, so layout still has a box.
pub(crate) fn open(source: ImageSource) -> (ImageClip, Option<ImageLoad>) {
    match source {
        ImageSource::Clip(clip) => (clip, None),
        ImageSource::Locator(locator) => open_locator(locator),
    }
}

fn open_locator(locator: String) -> (ImageClip, Option<ImageLoad>) {
    if is_http(&locator) {
        return (placeholder(), Some(start_http(locator)));
    }
    #[cfg(target_arch = "wasm32")]
    {
        return (placeholder(), Some(start_page(locator)));
    }
    #[cfg(not(target_arch = "wasm32"))]
    match read_file(&locator) {
        Ok(clip) => (clip, None),
        Err(error) => {
            log::warn!("{error}");
            (placeholder(), None)
        }
    }
}

fn placeholder() -> ImageClip {
    let pixels = crate::ImagePixels::from_rgba(1, 1, [0, 0, 0, 0])
        .expect("a 1 by 1 transparent pixel is valid");
    ImageClip::still(pixels)
}

fn is_http(locator: &str) -> bool {
    let locator = locator.trim();
    let head = locator.get(..8).unwrap_or(locator);
    head.eq_ignore_ascii_case("https://")
        || locator
            .get(..7)
            .unwrap_or(locator)
            .eq_ignore_ascii_case("http://")
}

fn decode_locator(locator: &str, bytes: &[u8]) -> Result<ImageClip, String> {
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(format!("image {locator} is larger than 32MB"));
    }
    ImageClip::decode(bytes).map_err(|error| format!("image {locator}: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(locator: &str) -> Result<ImageClip, String> {
    let path = filesystem_path(locator);
    let meta = std::fs::metadata(&path).map_err(|error| format!("image {locator}: {error}"))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!("image {locator} is larger than 32MB"));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("image {locator}: {error}"))?;
    decode_locator(locator, &bytes)
}

/// `file:///C:/a.png` and `file:///home/a.png` become native paths. Anything else is used as given.
#[cfg(not(target_arch = "wasm32"))]
fn filesystem_path(locator: &str) -> PathBuf {
    let trimmed = locator.trim();
    let Some(rest) = trimmed
        .strip_prefix("file://")
        .or_else(|| trimmed.strip_prefix("file:"))
    else {
        return PathBuf::from(trimmed);
    };
    let rest = rest
        .strip_prefix("//localhost")
        .or_else(|| rest.strip_prefix("//"))
        .unwrap_or(rest);
    #[cfg(windows)]
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    PathBuf::from(rest)
}

fn start_http(locator: String) -> ImageLoad {
    let slot = Arc::new(Mutex::new(None));
    let writer = Arc::clone(&slot);
    #[cfg(target_arch = "wasm32")]
    spawn_fetch(locator, writer);
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(move || {
            let result = fetch_http(&locator);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
    }
    ImageLoad { slot }
}

#[cfg(target_arch = "wasm32")]
fn start_page(locator: String) -> ImageLoad {
    let slot = Arc::new(Mutex::new(None));
    spawn_fetch(locator, Arc::clone(&slot));
    ImageLoad { slot }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_http(locator: &str) -> Result<ImageClip, String> {
    let mut current = locator.trim().to_owned();
    for _ in 0..4 {
        let parts = split_http_url(&current)?;
        let bytes = if parts.https {
            https_get(&parts)?
        } else {
            http_get(&parts)?
        };
        let (status, headers, body) = split_http_message(&bytes)?;
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = header(&headers, "location")
                .ok_or_else(|| format!("image {locator}: redirect without a location"))?;
            current = resolve_redirect(&parts, location);
            continue;
        }
        if status != 200 {
            return Err(format!("image {locator}: HTTP {status}"));
        }
        let payload = if header(&headers, "transfer-encoding")
            .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"))
        {
            decode_chunks(body)?
        } else {
            body.to_vec()
        };
        return decode_locator(locator, &payload);
    }
    Err(format!("image {locator}: too many redirects"))
}

#[cfg(not(target_arch = "wasm32"))]
struct HttpUrl {
    https: bool,
    host: String,
    port: u16,
    path: String,
}

#[cfg(not(target_arch = "wasm32"))]
fn split_http_url(locator: &str) -> Result<HttpUrl, String> {
    let (https, rest) = if let Some(rest) = strip_prefix_ignore_ascii_case(locator, "https://") {
        (true, rest)
    } else if let Some(rest) = strip_prefix_ignore_ascii_case(locator, "http://") {
        (false, rest)
    } else {
        return Err(format!("image {locator}: not an http url"));
    };
    if rest.is_empty() || rest.contains('@') {
        return Err(format!("image {locator}: unsupported url"));
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !host.contains(']') => {
            let port = port
                .parse::<u16>()
                .map_err(|_| format!("image {locator}: bad port"))?;
            (host.to_owned(), port)
        }
        _ => (authority.to_owned(), if https { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(format!("image {locator}: missing host"));
    }
    Ok(HttpUrl {
        https,
        host,
        port,
        path,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn request_bytes(url: &HttpUrl) -> Vec<u8> {
    let host = if (url.https && url.port == 443) || (!url.https && url.port == 80) {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    format!(
        "GET {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: cappy\r\nAccept: */*\r\n\r\n",
        url.path
    )
    .into_bytes()
}

#[cfg(not(target_arch = "wasm32"))]
fn https_get(url: &HttpUrl) -> Result<Vec<u8>, String> {
    let connector =
        native_tls::TlsConnector::new().map_err(|error| format!("image {}: {error}", url.host))?;
    let tcp = std::net::TcpStream::connect((url.host.as_str(), url.port))
        .map_err(|error| format!("image {}: {error}", url.host))?;
    let _ = tcp.set_read_timeout(Some(std::time::Duration::from_secs(20)));
    let _ = tcp.set_write_timeout(Some(std::time::Duration::from_secs(20)));
    let mut stream = connector
        .connect(&url.host, tcp)
        .map_err(|error| format!("image {}: {error}", url.host))?;
    stream
        .write_all(&request_bytes(url))
        .map_err(|error| format!("image {}: {error}", url.host))?;
    read_capped(&mut stream, &url.host)
}

#[cfg(not(target_arch = "wasm32"))]
fn http_get(url: &HttpUrl) -> Result<Vec<u8>, String> {
    let mut stream = std::net::TcpStream::connect((url.host.as_str(), url.port))
        .map_err(|error| format!("image {}: {error}", url.host))?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(20)));
    stream
        .write_all(&request_bytes(url))
        .map_err(|error| format!("image {}: {error}", url.host))?;
    read_capped(&mut stream, &url.host)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_capped(reader: &mut impl Read, host: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("image {host}: {error}"))?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len() as u64 + read as u64 > MAX_IMAGE_BYTES {
            return Err(format!("image {host} is larger than 32MB"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn split_http_message(bytes: &[u8]) -> Result<(u16, Vec<(String, String)>, &[u8]), String> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "image response has no header end".to_owned())?;
    let header = std::str::from_utf8(&bytes[..split])
        .map_err(|_| "image response headers are not utf-8".to_owned())?;
    let mut lines = header.split("\r\n");
    let status = lines.next().unwrap_or("");
    let code = status
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse::<u16>()
        .unwrap_or(0);
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    Ok((code, headers, &bytes[split + 4..]))
}

#[cfg(not(target_arch = "wasm32"))]
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_redirect(current: &HttpUrl, location: &str) -> String {
    if strip_prefix_ignore_ascii_case(location, "https://").is_some()
        || strip_prefix_ignore_ascii_case(location, "http://").is_some()
    {
        return location.to_owned();
    }
    let scheme = if current.https { "https" } else { "http" };
    let authority =
        if (current.https && current.port == 443) || (!current.https && current.port == 80) {
            current.host.clone()
        } else {
            format!("{}:{}", current.host, current.port)
        };
    if let Some(path) = location.strip_prefix('/') {
        return format!("{scheme}://{authority}/{path}");
    }
    let directory = current
        .path
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");
    format!("{scheme}://{authority}{directory}/{location}")
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_chunks(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| "image response has a broken chunk".to_owned())?;
        let line = std::str::from_utf8(&body[..line_end])
            .map_err(|_| "image response has a broken chunk size".to_owned())?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "image response has a broken chunk size".to_owned())?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size + 2 {
            return Err("image response ended inside a chunk".to_owned());
        }
        if out.len() as u64 + size as u64 > MAX_IMAGE_BYTES {
            return Err("image is larger than 32MB".to_owned());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

#[cfg(target_arch = "wasm32")]
fn spawn_fetch(locator: String, writer: Arc<Mutex<Option<Result<ImageClip, String>>>>) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_page(&locator).await;
        if let Ok(mut guard) = writer.lock() {
            *guard = Some(result);
        }
    });
}

#[cfg(target_arch = "wasm32")]
async fn fetch_page(locator: &str) -> Result<ImageClip, String> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| "image load has no browser window".to_owned())?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(locator))
        .await
        .map_err(js_message)?;
    let response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| format!("image {locator}: fetch did not return a response"))?;
    if !response.ok() {
        return Err(format!("image {locator}: HTTP {}", response.status()));
    }
    let buffer = response
        .array_buffer()
        .map_err(|error| format!("image {locator}: {}", js_message(error)))?;
    let buffer = wasm_bindgen_futures::JsFuture::from(buffer)
        .await
        .map_err(js_message)?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    decode_locator(locator, &bytes)
}

#[cfg(target_arch = "wasm32")]
fn js_message(value: wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "image request failed".to_owned())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{decode_chunks, fetch_http, split_http_url};

    #[test]
    fn an_https_url_keeps_the_path_and_query() {
        let url = split_http_url("https://example.com/a%20b.png?x=1").expect("url");
        assert!(url.https);
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 443);
        assert_eq!(url.path, "/a%20b.png?x=1");
    }

    #[test]
    fn chunked_bodies_join_in_order() {
        let raw = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(decode_chunks(raw).expect("chunks"), b"Wikipedia");
    }

    #[test]
    fn a_local_http_server_decodes_the_response_body() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let png = include_bytes!("../fixtures/sample.png").to_vec();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut ignored = [0_u8; 1024];
            let _ = stream.read(&mut ignored);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            );
            stream.write_all(header.as_bytes()).expect("header");
            stream.write_all(&png).expect("body");
        });

        let clip = fetch_http(&format!("http://127.0.0.1:{port}/sample.png")).expect("fetch");
        assert_eq!(clip.frame_pixels(0).width(), 32);
        assert_eq!(&clip.frame_pixels(0).rgba()[..4], &[255, 0, 0, 128]);
    }
}
