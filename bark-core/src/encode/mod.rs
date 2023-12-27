pub mod opus;
pub mod pcm;

use core::fmt::Display;

use bark_protocol::types::AudioPacketFormat;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NewEncoderError {
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("output buffer too small, need at least {need} bytes")]
    OutputBufferTooSmall { need: usize },
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

pub trait Encode: Display + Send {
    fn header_format(&self) -> AudioPacketFormat;
    fn encode_packet(&mut self, samples: &[f32], out: &mut [u8]) -> Result<usize, EncodeError>;
}
