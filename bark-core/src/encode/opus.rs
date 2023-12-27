use core::fmt::{self, Display};

use bark_protocol::{types::AudioPacketFormat, SAMPLE_RATE};

use super::{Encode, EncodeError, NewEncoderError};

pub struct OpusEncoder {
    opus: opus::Encoder,
}

impl OpusEncoder {
    pub fn new() -> Result<Self, NewEncoderError> {
        let mut opus = opus::Encoder::new(
            SAMPLE_RATE.0,
            opus::Channels::Stereo,
            opus::Application::Audio,
        )?;

        opus.set_inband_fec(true)?;
        opus.set_packet_loss_perc(50)?;

        Ok(OpusEncoder { opus })
    }
}

impl Display for OpusEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opus")
    }
}

impl Encode for OpusEncoder {
    fn header_format(&self) -> AudioPacketFormat {
        AudioPacketFormat::OPUS
    }

    fn encode_packet(&mut self, samples: &[f32], out: &mut [u8]) -> Result<usize, EncodeError> {
        let len = self.opus.encode_float(samples, out)?;

        log::debug!("opus encode: frames={} -> bytes={}", samples.len() / 2, len);

        Ok(len)
    }
}
