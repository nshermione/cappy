//! Decode loop. It pulls packets and writes pictures. Seek and playlists stay in the adapter.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use crate::decode::FfmpegDecoder;
use crate::packet::{EncodedPacket, PacketSource, Pull, StreamConfig};
use crate::session::Session;
use crate::sound::{self, AudioDecoder};

/// Audio packets kept while the device buffer is full, so the picture queue can still move.
const PARKED_AUDIO: usize = 256;
/// Video packets held while two pictures are already queued, so audio packets behind them
/// can still reach the device. Without this the clock never starts and the queue never drains.
const STAGED_VIDEO: usize = 16;

pub(crate) fn run(
    mut source: Box<dyn PacketSource>,
    config: Option<StreamConfig>,
    session: Arc<Session>,
) {
    let mut decoder = match config {
        Some(config) => match FfmpegDecoder::open(&config) {
            Ok(decoder) => Some(decoder),
            Err(_) => {
                session.mark_finished();
                return;
            }
        },
        None => None,
    };
    if source.can_rewind() && session.wants_previews() {
        let Some(decoder) = decoder.as_mut() else {
            session.mark_finished();
            return;
        };
        collect_previews(source.as_mut(), decoder, &session);
        source.rewind();
        decoder.reset();
    }
    let mut audio_decoder: Option<AudioDecoder> = None;
    if session.wants_audio() {
        sound::open_speaker(&session);
    }
    let mut parked: VecDeque<EncodedPacket> = VecDeque::new();
    let mut staged: VecDeque<EncodedPacket> = VecDeque::new();
    let mut last_published: Option<Duration> = None;
    let mut saw_end = false;
    loop {
        if session.stopped() {
            break;
        }
        pump_audio(&mut parked, &mut audio_decoder, source.as_ref(), &session);
        if !session.playing() && !session.seek_pending() {
            session.wait();
            continue;
        }
        if session.seek_pending() {
            staged.clear();
        }
        let decode_video =
            !session.queue_full() || session.seek_pending() || !session.holds_picture_queue();
        let staging_full = !decode_video
            && (staged.len() >= STAGED_VIDEO
                || (session.pcm_full() && parked.len() >= PARKED_AUDIO));
        if staging_full {
            session.wait_timeout(Duration::from_millis(50));
            continue;
        }
        let max_edge = session.max_edge();
        let pulled = if decode_video {
            staged
                .pop_front()
                .map(Pull::Packet)
                .unwrap_or_else(|| source.pull())
        } else {
            source.pull()
        };
        let frames = match pulled {
            Pull::Packet(packet) if packet.audio => {
                saw_end = false;
                park_audio(&mut parked, packet);
                pump_audio(&mut parked, &mut audio_decoder, source.as_ref(), &session);
                Vec::new()
            }
            Pull::Packet(packet) if !decode_video => {
                saw_end = false;
                staged.push_back(packet);
                Vec::new()
            }
            Pull::Packet(packet) => {
                saw_end = false;
                let Some(decoder) = ready_decoder(&mut decoder, source.as_ref()) else {
                    session.mark_finished();
                    break;
                };
                decoder.push(&packet, max_edge)
            }
            Pull::Reset => {
                if let Some(decoder) = decoder.as_mut() {
                    decoder.reset();
                }
                if let Some(decoder) = audio_decoder.as_mut() {
                    decoder.reset();
                }
                parked.clear();
                staged.clear();
                session.clear_pcm(session.anchor_at());
                last_published = None;
                Vec::new()
            }
            Pull::Idle => {
                session.wait();
                continue;
            }
            Pull::Ended if saw_end => {
                session.clear_accurate();
                session.mark_finished();
                session.wait();
                continue;
            }
            Pull::Ended => {
                session.clear_accurate();
                saw_end = true;
                match decoder.as_mut() {
                    Some(decoder) => decoder.finish(max_edge),
                    None => Vec::new(),
                }
            }
        };
        let grows = decoder.as_ref().map(FfmpegDecoder::grows).unwrap_or(0);
        for frame in frames {
            session.publish(frame, grows, &mut last_published);
        }
    }
}

fn park_audio(parked: &mut VecDeque<EncodedPacket>, packet: EncodedPacket) {
    parked.push_back(packet);
    while parked.len() > PARKED_AUDIO {
        parked.pop_front();
    }
}

fn pump_audio(
    parked: &mut VecDeque<EncodedPacket>,
    audio_decoder: &mut Option<AudioDecoder>,
    source: &dyn PacketSource,
    session: &Session,
) {
    if !session.audio_live() {
        return;
    }
    while !session.pcm_full() {
        let Some(packet) = parked.pop_front() else {
            break;
        };
        let Some(decoder) = ready_audio(audio_decoder, source, session) else {
            parked.push_front(packet);
            break;
        };
        let pcm = decoder.push(&packet);
        session.push_pcm(&pcm);
    }
}

fn ready_audio<'a>(
    slot: &'a mut Option<AudioDecoder>,
    source: &dyn PacketSource,
    session: &Session,
) -> Option<&'a mut AudioDecoder> {
    if slot.is_none() {
        let facts = source.audio_facts()?;
        let (channels, rate) = session.audio_format()?;
        *slot = AudioDecoder::open(&facts, rate, channels).ok();
    }
    slot.as_mut()
}

fn ready_decoder<'a>(
    decoder: &'a mut Option<FfmpegDecoder>,
    source: &dyn PacketSource,
) -> Option<&'a mut FfmpegDecoder> {
    if decoder.is_none() {
        *decoder = source
            .config()
            .and_then(|config| FfmpegDecoder::open(&config).ok());
    }
    decoder.as_mut()
}

fn collect_previews(source: &mut dyn PacketSource, decoder: &mut FfmpegDecoder, session: &Session) {
    loop {
        match source.pull() {
            Pull::Packet(packet) if packet.key => {
                for frame in decoder.push(&packet, 160) {
                    session.store_preview(frame);
                }
                decoder.reset();
            }
            Pull::Packet(_) => {}
            Pull::Reset => decoder.reset(),
            Pull::Idle | Pull::Ended => break,
        }
    }
}
