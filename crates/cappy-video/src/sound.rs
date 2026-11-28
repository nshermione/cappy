//! Audio decode and the output device. Both stay closed unless the opener asked for sound.
//! The device stream is not movable, so it stays on its own thread. Play and pause are messages.

use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Weak};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_next as ffmpeg;

use crate::packet::{AudioCodec, AudioFacts, EncodedPacket};
use crate::session::Session;

pub(crate) struct AudioDecoder {
    decoder: ffmpeg::decoder::Audio,
    resampler: Option<ffmpeg::software::resampling::context::Context>,
    out_rate: u32,
    out_channels: u16,
    eof_sent: bool,
}

impl AudioDecoder {
    pub(crate) fn open(
        facts: &AudioFacts,
        out_rate: u32,
        out_channels: u16,
    ) -> Result<Self, String> {
        let codec_id = match facts.codec {
            AudioCodec::Aac => ffmpeg::codec::Id::AAC,
            AudioCodec::Mp3 => ffmpeg::codec::Id::MP3,
        };
        let codec = ffmpeg::decoder::find(codec_id).ok_or_else(|| "no audio decoder".to_owned())?;
        let mut context = ffmpeg::codec::context::Context::new_with_codec(codec);
        if !facts.extradata.is_empty() {
            context.set_extradata(Some(facts.extradata.as_slice()));
        }
        let decoder = context
            .decoder()
            .audio()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            decoder,
            resampler: None,
            out_rate: out_rate.max(1),
            out_channels,
            eof_sent: false,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        self.eof_sent = false;
    }

    pub(crate) fn push(&mut self, packet: &EncodedPacket) -> Vec<f32> {
        if self.eof_sent {
            self.reset();
        }
        let mut av = ffmpeg::Packet::copy(&packet.data);
        av.set_pts(packet.pts);
        if self.decoder.send_packet(&av).is_err() {
            return Vec::new();
        }
        self.drain()
    }

    fn drain(&mut self) -> Vec<f32> {
        let mut frame = ffmpeg::frame::Audio::empty();
        let mut pcm = Vec::new();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let Some(resampled) = self.resample(&frame) else {
                continue;
            };
            pcm.extend_from_slice(&resampled);
        }
        pcm
    }

    fn resample(&mut self, frame: &ffmpeg::frame::Audio) -> Option<Vec<f32>> {
        self.ensure_resampler(frame)?;
        let resampler = self.resampler.as_mut()?;
        let layout = output_layout(self.out_channels);
        let grown =
            frame.samples() as u64 * u64::from(self.out_rate) / u64::from(frame.rate().max(1)) + 32;
        let mut output = ffmpeg::frame::Audio::new(
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            grown as usize,
            layout,
        );
        resampler.run(frame, &mut output).ok()?;
        Some(output.plane::<f32>(0).to_vec())
    }

    fn ensure_resampler(&mut self, frame: &ffmpeg::frame::Audio) -> Option<()> {
        if self.resampler.is_some() {
            return Some(());
        }
        let input_layout = if frame.channel_layout().channels() == 0 {
            output_layout(frame.channels())
        } else {
            frame.channel_layout()
        };
        let rate = frame.rate().max(1);
        self.resampler = ffmpeg::software::resampling::context::Context::get(
            frame.format(),
            input_layout,
            rate,
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            output_layout(self.out_channels),
            self.out_rate,
        )
        .ok();
        self.resampler.as_ref().map(|_| ())
    }
}

fn output_layout(channels: u16) -> ffmpeg::ChannelLayout {
    if channels <= 1 {
        ffmpeg::ChannelLayout::MONO
    } else {
        ffmpeg::ChannelLayout::STEREO
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SpeakerCmd {
    Play,
    Pause,
}

/// Opens the default output device on a thread that keeps the stream.
/// A missing device leaves the picture clock on wall time.
pub(crate) fn open_speaker(session: &Arc<Session>) -> bool {
    if session.manual_output() {
        return true;
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let weak = Arc::downgrade(session);
    let playing = session.playing() && !session.muted();
    std::thread::spawn(move || speaker_thread(weak, playing, ready_tx, cmd_rx));
    let Ok(Some((channels, rate))) = ready_rx.recv() else {
        return false;
    };
    session.prepare_audio(channels, rate);
    session.store_speaker(cmd_tx);
    true
}

fn speaker_thread(
    session: Weak<Session>,
    playing: bool,
    ready: SyncSender<Option<(u16, u32)>>,
    commands: Receiver<SpeakerCmd>,
) {
    let Ok(device) = default_device() else {
        let _ = ready.send(None);
        return;
    };
    let Some((config, channels, rate)) = f32_config(&device) else {
        let _ = ready.send(None);
        return;
    };
    let callback_session = Weak::clone(&session);
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let Some(session) = callback_session.upgrade() else {
                data.fill(0.0);
                return;
            };
            session.render_audio(data);
        },
        |_error| {},
        None,
    );
    let Ok(stream) = stream else {
        let _ = ready.send(None);
        return;
    };
    let started = if playing {
        stream.play().is_ok()
    } else {
        stream.pause().is_ok()
    };
    if !started {
        let _ = ready.send(None);
        return;
    }
    if ready.send(Some((channels, rate))).is_err() {
        return;
    }
    while let Ok(command) = commands.recv() {
        match command {
            SpeakerCmd::Play => {
                let _ = stream.play();
            }
            SpeakerCmd::Pause => {
                let _ = stream.pause();
            }
        }
    }
}

fn default_device() -> Result<cpal::Device, String> {
    cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no audio device".to_owned())
}

fn f32_config(device: &cpal::Device) -> Option<(cpal::StreamConfig, u16, u32)> {
    let configs = device.supported_output_configs().ok()?;
    for range in configs {
        let channels = range.channels();
        if channels != 1 && channels != 2 {
            continue;
        }
        if range.sample_format() != cpal::SampleFormat::F32 {
            continue;
        }
        let picked = range.with_max_sample_rate();
        return Some((picked.config(), picked.channels(), picked.sample_rate().0));
    }
    None
}
