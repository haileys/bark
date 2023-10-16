use std::cmp;
use std::sync::Arc;

use bark_network::ProtocolSocket;
use bark_protocol::FRAMES_PER_PACKET;
use bark_protocol::packet::Audio;
use bark_protocol::time::{Timestamp, SampleDuration};
use bark_protocol::types::{AudioFrameF32, AudioPacketHeader, SessionId};

pub struct Encode {
    protocol: Arc<ProtocolSocket>,
    codec: Box<dyn Encoding>,
    buffer: [AudioFrameF32; FRAMES_PER_PACKET],
    pts: Option<Timestamp>,
    pos: usize,
    sid: SessionId,
    seq: u64,
}

impl Encode {
    pub fn new(protocol: Arc<ProtocolSocket>, codec: Box<dyn Encoding>, sid: SessionId) -> Self {
        Encode {
            protocol,
            codec,
            buffer: [AudioFrameF32::zero(); FRAMES_PER_PACKET],
            pts: None,
            pos: 0,
            sid,
            seq: 0,
        }
    }

    fn send_full_buffer(&mut self) {
        let pts = self.pts.take().unwrap();
        self.pos = 0;

        let seq = self.seq;
        self.seq += 1;

        let header = StreamHeader {
            sid: self.sid,
            seq,
            pts,
        };

        let Some(packet) = self.codec.encode(&header, &self.buffer) else {
            return;
        };

        let _ = self.protocol.broadcast(packet.as_packet());
    }

    fn write_partial(&mut self, pts: Timestamp, data: &[AudioFrameF32]) -> usize {
        let remaining = self.buffer.len() - self.pos;
        let n = cmp::min(remaining, data.len());
        self.buffer[self.pos..][..n].copy_from_slice(&data[..n]);

        // set pts if this is the start of a frame:
        if self.pos == 0 {
            self.pts = Some(pts);
        }

        // increment pos
        self.pos += n;

        // send full buffer if this is the end of a frame:
        if self.pos == self.buffer.len() {
            self.send_full_buffer();
        }

        n
    }

    pub fn write(&mut self, mut pts: Timestamp, mut data: &[AudioFrameF32]) {
        while data.len() > 0 {
            let n = self.write_partial(pts, data);
            data = &data[n..];
            pts += SampleDuration::from_frame_buffer_offset(n);
        }
    }
}

pub struct StreamHeader {
    sid: SessionId,
    seq: u64,
    pts: Timestamp,
}

pub trait Encoding {
    fn encode(&mut self, header: &StreamHeader, data: &[AudioFrameF32]) -> Option<Audio>;
}

pub struct F32;

impl Encoding for F32 {
    fn encode(&mut self, header: &StreamHeader, data: &[AudioFrameF32]) -> Option<Audio> {
        let mut audio = Audio::write().ok()?;
        audio.write(data);
        Some(audio.finalize(AudioPacketHeader {
            sid: header.sid,
            seq: header.seq,
            pts: header.pts.to_micros_lossy(),
            dts: bark_util::time::now(),
        }))
    }
}

/*
struct Opus {
    opus: opus::Encoder,
    packet: [u8; MAX_PACKET_SIZE],
}

impl Opus {
    pub fn new() -> Result<Self, opus::Error> {
        let opus = opus::Encoder::new(
            SAMPLE_RATE.0,
            opus::Channels::Stereo,
            opus::Application::Audio,
        )?;

        Ok(Opus { opus, packet: [0; MAX_PACKET_SIZE] })
    }
}

impl Encoding for Opus {
    fn encode(&mut self, data: &[AudioFrameF32]) -> Option<&[u8]> {
        let input = AudioFrameF32::as_interleaved_slice(data);
        match self.opus.encode_float(input, &mut self.packet) {
            Ok(n) => {
                log::trace!("encoded opus frame: {n} bytes");
                Some(&self.packet[0..n])
            }
            Err(e) => {
                log::warn!("error encoding opus frame: {e:?}");
                None
            }
        }
    }
}
*/
