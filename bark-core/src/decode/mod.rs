pub mod opus;
pub mod pcm;

use core::fmt::Display;

use bark_protocol::packet::Audio;
use thiserror::Error;

use bark_protocol::types::{AudioPacketHeader, AudioPacketFormat};
use bark_protocol::SAMPLES_PER_PACKET;

#[derive(Debug, Error)]
pub enum NewDecoderError {
    #[error("unknown format in audio header: {0:?}")]
    UnknownFormat(AudioPacketFormat),
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("wrong length: {length}, expected: {expected}")]
    WrongLength { length: usize, expected: usize },
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

pub struct Decoder {
    decode: DecodeFormat,
}

pub type SampleBuffer = [f32; SAMPLES_PER_PACKET];

impl Decoder {
    pub fn new(header: &AudioPacketHeader) -> Result<Self, NewDecoderError> {
        let decode = match header.format {
            AudioPacketFormat::S16LE => DecodeFormat::S16LE(pcm::S16LEDecoder),
            AudioPacketFormat::F32LE => DecodeFormat::F32LE(pcm::F32LEDecoder),
            AudioPacketFormat::OPUS => DecodeFormat::Opus(opus::OpusDecoder::new()?),
            format => { return Err(NewDecoderError::UnknownFormat(format)) }
        };

        Ok(Decoder { decode })
    }

    pub fn describe(&self) -> impl Display + '_ {
        &self.decode as &dyn Display
    }

    pub fn decode(&mut self, packet: &Audio, out: &mut SampleBuffer) -> Result<(), DecodeError> {
        self.decode.decode_packet(packet.buffer_bytes(), out)
    }
}

trait Decode: Display {
    fn decode_packet(&mut self, bytes: &[u8], out: &mut SampleBuffer) -> Result<(), DecodeError>;
}

enum DecodeFormat {
    S16LE(pcm::S16LEDecoder),
    F32LE(pcm::F32LEDecoder),
    Opus(opus::OpusDecoder),
}

impl Decode for DecodeFormat {
    fn decode_packet(&mut self, bytes: &[u8], out: &mut SampleBuffer) -> Result<(), DecodeError> {
        match self {
            DecodeFormat::S16LE(dec) => dec.decode_packet(bytes, out),
            DecodeFormat::F32LE(dec) => dec.decode_packet(bytes, out),
            DecodeFormat::Opus(dec) => dec.decode_packet(bytes, out),
        }
    }
}

impl Display for DecodeFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeFormat::S16LE(dec) => dec.fmt(f),
            DecodeFormat::F32LE(dec) => dec.fmt(f),
            DecodeFormat::Opus(dec) => dec.fmt(f),
        }
    }
}
