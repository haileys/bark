pub mod opus;
pub mod pcm;

use core::fmt::Display;

use thiserror::Error;

use bark_protocol::FRAMES_PER_PACKET;
use bark_protocol::packet::Audio;
use bark_protocol::types::{AudioPacketHeader, AudioPacketFormat};

use crate::audio::{Frame, SampleFormat};

#[derive(Debug, Error)]
pub enum NewDecoderError {
    #[error("unknown format in audio header: {0:?}")]
    UnknownFormat(AudioPacketFormat),
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("wrong byte length: {length}, expected: {expected}")]
    WrongLength { length: usize, expected: usize },
    #[error("wrong frame count: {frames}, expected: {expected}")]
    WrongFrameCount { frames: usize, expected: usize },
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

pub struct Decoder {
    decode: DecodeFormat,
}

pub type FrameBuffer<Sample: SampleFormat> = [Sample::Frame; FRAMES_PER_PACKET];

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

    pub fn decode(&mut self, packet: Option<&Audio>, out: &mut FrameBuffer<f32>) -> Result<(), DecodeError> {
        let bytes = packet.map(|packet| packet.buffer_bytes());
        self.decode.decode_packet(bytes, out)
    }
}

trait Decode<Sample: SampleFormat>: Display {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer<Sample>) -> Result<(), DecodeError>;
}

enum DecodeFormat {
    S16LE(pcm::S16LEDecoder),
    F32LE(pcm::F32LEDecoder),
    Opus(opus::OpusDecoder<f32>),
}

impl<Sample: SampleFormat> Decode<Sample> for DecodeFormat {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer<Sample>) -> Result<(), DecodeError> {
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
