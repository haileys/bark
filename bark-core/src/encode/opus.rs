use core::fmt::{self, Display};

use bark_protocol::{types::AudioPacketFormat, SAMPLE_RATE};

use crate::audio::{self, Frames, F32, S16};
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
        opus.set_bitrate(opus::Bitrate::Max)?;

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

    fn encode_packet(&mut self, frames: Frames, out: &mut [u8]) -> Result<usize, EncodeError> {
        let n = match frames {
            Frames::S16(frames) => self.opus.encode(audio::as_interleaved::<S16>(frames), out)?,
            Frames::F32(frames) => self.opus.encode_float(audio::as_interleaved::<F32>(frames), out)?,
        };

        Ok(n)
    }
}
