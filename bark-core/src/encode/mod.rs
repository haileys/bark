#[cfg(feature = "opus")]
pub mod opus;

pub mod pcm;

use core::fmt::Display;

use bark_protocol::types::AudioPacketFormat;
use thiserror::Error;

use crate::audio::Frame;

#[derive(Debug, Error)]
pub enum NewEncoderError {
    #[cfg(feature = "opus")]
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("output buffer too small, need at least {need} bytes")]
    OutputBufferTooSmall { need: usize },
    #[cfg(feature = "opus")]
    #[error("opus codec error: {0}")]
    Opus(#[from] ::opus::Error),
}

pub trait Encode: Display + Send {
    fn header_format(&self) -> AudioPacketFormat;
    fn encode_packet(&mut self, frames: &[Frame], out: &mut [u8]) -> Result<usize, EncodeError>;
}
