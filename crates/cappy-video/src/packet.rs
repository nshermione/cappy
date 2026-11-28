//! Encoded packets. An adapter yields these. The decoder never sees a playlist or a file path.

use std::collections::VecDeque;

/// What the decoder needs before the first packet. This is not an FFmpeg type.
#[derive(Clone, Debug)]
pub(crate) struct StreamConfig {
    pub codec: VideoCodec,
    pub time_base_num: i32,
    pub time_base_den: i32,
    pub extradata: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VideoCodec {
    H264,
}

/// One encoded access unit. `discontinuity` is not used here; a seek is [`Pull::Reset`].
#[derive(Clone, Debug)]
pub(crate) struct EncodedPacket {
    pub data: Vec<u8>,
    pub pts: Option<i64>,
    #[allow(dead_code)]
    pub key: bool,
    /// An audio packet. The video decoder never sees one.
    pub audio: bool,
}

/// Enough to open an audio decoder. Unknown codecs stay `None` and are not played.
#[derive(Clone, Debug)]
pub(crate) struct AudioFacts {
    pub codec: AudioCodec,
    pub rate: u32,
    pub channels: u16,
    pub extradata: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioCodec {
    Aac,
    Mp3,
}

pub(crate) enum Pull {
    Packet(EncodedPacket),
    /// Drop decoded pictures. The next packet starts a new run.
    Reset,
    #[allow(dead_code)]
    Idle,
    Ended,
}

pub(crate) trait PacketSource: Send {
    fn pull(&mut self) -> Pull;

    /// When true, the worker reads keyframes once, then calls [`rewind`](Self::rewind).
    fn can_rewind(&self) -> bool {
        false
    }

    fn rewind(&mut self) {}

    /// Codec facts learned from the first segment. `None` until that segment has been demuxed.
    fn config(&self) -> Option<StreamConfig> {
        None
    }

    fn audio_facts(&self) -> Option<AudioFacts> {
        None
    }
}

/// Packets already in memory. Used to prove the decoder does not depend on the file adapter.
#[cfg(test)]
pub(crate) struct ListSource {
    packets: VecDeque<EncodedPacket>,
    ended: bool,
    audio: Option<AudioFacts>,
}

#[cfg(test)]
impl ListSource {
    pub(crate) fn new(packets: Vec<EncodedPacket>) -> Self {
        Self::with_audio(packets, None)
    }

    pub(crate) fn with_audio(packets: Vec<EncodedPacket>, audio: Option<AudioFacts>) -> Self {
        Self {
            packets: packets.into(),
            ended: false,
            audio,
        }
    }
}

#[cfg(test)]
impl PacketSource for ListSource {
    fn pull(&mut self) -> Pull {
        if let Some(packet) = self.packets.pop_front() {
            return Pull::Packet(packet);
        }
        if self.ended {
            return Pull::Idle;
        }
        self.ended = true;
        Pull::Ended
    }

    fn audio_facts(&self) -> Option<AudioFacts> {
        self.audio.clone()
    }
}
