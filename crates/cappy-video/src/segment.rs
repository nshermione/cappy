//! Segment pipeline. A playlist only creates jobs. Fetch, decrypt, and demux run in that order.
//! The decoder never reads the playlist.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::file::demux_media;
use crate::packet::{AudioFacts, EncodedPacket, PacketSource, Pull};
use crate::session::Session;

#[derive(Clone, Debug)]
pub(crate) struct SegmentJob {
    pub path: PathBuf,
    pub start: Duration,
    pub duration: Duration,
    pub key: Option<SegmentKey>,
    pub delay: Duration,
    /// DASH initialization segment. Fetched and prepended before demux.
    pub init: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct SegmentKey {
    pub method: String,
    pub uri: PathBuf,
    pub iv: [u8; 16],
}

pub(crate) enum NextSegment {
    Job(SegmentJob),
    /// The live edge is caught up. Try again after the playlist changes.
    Wait,
    Done,
}

pub(crate) trait SegmentSource: Send {
    fn next_segment(&mut self) -> NextSegment;
    fn seek(&mut self, at: Duration);

    /// The segment at `at` failed. Switch down and try that time again.
    /// The default ends the stream.
    fn recover(&mut self, _at: Duration) -> bool {
        false
    }

    /// The segment after the one just returned. The pipeline downloads it while
    /// the current packets play, so the next fetch does not sit on the decode thread.
    fn upcoming(&self) -> Option<PathBuf> {
        None
    }

    /// A finished fetch. Playlists may switch variant. The default keeps the current one.
    fn observe(
        &mut self,
        _fetch: Duration,
        _segment: Duration,
        _buffer_full: bool,
        _max_edge: u32,
    ) {
    }
}

const SEGMENT_CACHE_BYTES: usize = 96 * 1024 * 1024;

struct KeptSegment {
    config: crate::packet::StreamConfig,
    packets: Vec<EncodedPacket>,
    bytes: usize,
}

pub(crate) struct Pipeline<S> {
    source: S,
    packets: VecDeque<EncodedPacket>,
    session: Arc<Session>,
    config: Option<crate::packet::StreamConfig>,
    audio: Option<AudioFacts>,
    kept: HashMap<PathBuf, KeptSegment>,
    kept_order: VecDeque<PathBuf>,
    kept_bytes: usize,
}

impl<S> Pipeline<S> {
    pub(crate) fn new(source: S, session: Arc<Session>) -> Self {
        Self {
            source,
            packets: VecDeque::new(),
            session,
            config: None,
            audio: None,
            kept: HashMap::new(),
            kept_order: VecDeque::new(),
            kept_bytes: 0,
        }
    }

    fn recall(&self, path: &Path) -> Option<(crate::packet::StreamConfig, Vec<EncodedPacket>)> {
        self.kept
            .get(path)
            .map(|kept| (kept.config.clone(), kept.packets.clone()))
    }

    fn remember(
        &mut self,
        path: &Path,
        config: &crate::packet::StreamConfig,
        packets: &[EncodedPacket],
    ) {
        if self.kept.contains_key(path) {
            return;
        }
        let bytes = packets.iter().map(|packet| packet.data.len()).sum();
        self.kept_bytes = self.kept_bytes.saturating_add(bytes);
        self.kept_order.push_back(path.to_path_buf());
        self.kept.insert(
            path.to_path_buf(),
            KeptSegment {
                config: config.clone(),
                packets: packets.to_vec(),
                bytes,
            },
        );
        while self.kept_bytes > SEGMENT_CACHE_BYTES && self.kept_order.len() > 1 {
            let Some(old) = self.kept_order.pop_front() else {
                break;
            };
            if let Some(removed) = self.kept.remove(&old) {
                self.kept_bytes = self.kept_bytes.saturating_sub(removed.bytes);
            }
        }
    }

    fn prefetch_upcoming(&self)
    where
        S: SegmentSource,
    {
        if let Some(path) = self.source.upcoming() {
            crate::locator::prefetch(&path);
        }
    }
}

impl<S: SegmentSource> PacketSource for Pipeline<S> {
    fn pull(&mut self) -> Pull {
        if let Some(at) = self.session.take_seek() {
            self.packets.clear();
            self.source.seek(at);
            return Pull::Reset;
        }
        if let Some(packet) = self.packets.pop_front() {
            return Pull::Packet(packet);
        }
        match self.source.next_segment() {
            NextSegment::Job(job) => {
                if let Some((config, packets)) = self.recall(&job.path) {
                    if self.config.is_none() {
                        self.config = Some(config);
                    }
                    self.prefetch_upcoming();
                    self.packets.extend(packets);
                    return self.pull();
                }
                let started = std::time::Instant::now();
                if !job.delay.is_zero() {
                    std::thread::sleep(job.delay);
                }
                match load_job(&job, self.session.wants_audio()) {
                    Ok(demuxed) => {
                        self.remember(&job.path, &demuxed.config, &demuxed.packets);
                        if self.config.is_none() {
                            self.config = Some(demuxed.config);
                        }
                        if self.audio.is_none() {
                            self.audio = demuxed.audio;
                        }
                        self.source.observe(
                            started.elapsed(),
                            job.duration,
                            self.session.queue_full(),
                            self.session.max_edge(),
                        );
                        self.prefetch_upcoming();
                        self.packets.extend(demuxed.packets);
                        self.pull()
                    }
                    Err(_) => {
                        if self.source.recover(job.start) {
                            return self.pull();
                        }
                        Pull::Ended
                    }
                }
            }
            NextSegment::Wait => Pull::Idle,
            NextSegment::Done => Pull::Ended,
        }
    }

    fn config(&self) -> Option<crate::packet::StreamConfig> {
        self.config.clone()
    }

    fn audio_facts(&self) -> Option<AudioFacts> {
        self.audio.clone()
    }
}

fn load_job(job: &SegmentJob, keep_audio: bool) -> Result<crate::file::Demuxed, String> {
    open_container(job, keep_audio)
}

fn open_container(job: &SegmentJob, keep_audio: bool) -> Result<crate::file::Demuxed, String> {
    let remote = crate::locator::is_remote(&job.path.to_string_lossy());
    let bytes = fetch(&job.path)?;
    let clear = decrypt(&bytes, job.key.as_ref())?;
    let temporary = if remote || job.key.is_some() || job.init.is_some() {
        let mut body = Vec::new();
        if let Some(init) = &job.init {
            body.extend(fetch(init)?);
        }
        body.extend(clear);
        let name = job
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("segment")
            .replace('/', "_");
        let path =
            std::env::temp_dir().join(format!("cappy-segment-{}-{name}", next_segment_file()));
        std::fs::write(&path, &body).map_err(|error| error.to_string())?;
        Some(path)
    } else {
        None
    };
    let demux_at = temporary.as_deref().unwrap_or(job.path.as_path());
    let demuxed = demux_media(demux_at, keep_audio);
    if let Some(path) = temporary {
        let _ = std::fs::remove_file(path);
    }
    let mut demuxed = demuxed?;
    rebase(
        &mut demuxed.packets,
        job.start,
        demuxed.config.time_base_num,
        demuxed.config.time_base_den,
    );
    Ok(demuxed)
}

fn rebase(packets: &mut [EncodedPacket], start: Duration, num: i32, den: i32) {
    let Some(origin) = packets
        .iter()
        .filter(|packet| !packet.audio)
        .filter_map(|packet| packet.pts)
        .min()
    else {
        return;
    };
    let shift = ticks(start, num, den);
    for packet in packets.iter_mut().filter(|packet| !packet.audio) {
        if let Some(pts) = packet.pts.as_mut() {
            *pts = pts.saturating_sub(origin).saturating_add(shift);
        }
    }
}

fn ticks(at: Duration, num: i32, den: i32) -> i64 {
    if num == 0 {
        return 0;
    }
    (at.as_secs_f64() * f64::from(den) / f64::from(num)) as i64
}

fn fetch(path: &Path) -> Result<Vec<u8>, String> {
    crate::locator::read_bytes(path)
}

fn next_segment_file() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn decrypt(bytes: &[u8], key: Option<&SegmentKey>) -> Result<Vec<u8>, String> {
    match key {
        None => Ok(bytes.to_vec()),
        Some(key) if key.method == "NONE" => Ok(bytes.to_vec()),
        Some(key) if key.method == "AES-128" => decrypt_aes128(bytes, key),
        Some(key) => Err(format!("unsupported segment encryption {}", key.method)),
    }
}

fn decrypt_aes128(bytes: &[u8], key: &SegmentKey) -> Result<Vec<u8>, String> {
    use aes::Aes128;
    use cbc::Decryptor;
    use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

    let secret = crate::locator::read_bytes(&key.uri)?;
    let Some(secret) = secret.as_slice().try_into().ok() else {
        return Err("AES-128 key must be 16 bytes".to_owned());
    };
    let secret: [u8; 16] = secret;
    let decryptor = Decryptor::<Aes128>::new(&secret.into(), &key.iv.into());
    decryptor
        .decrypt_padded_vec_mut::<Pkcs7>(bytes)
        .map_err(|_| "AES-128 segment did not decrypt".to_owned())
}
