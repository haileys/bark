use core::fmt::{self, Display};

use bark_protocol::SAMPLE_RATE;

use crate::audio;

use super::{Decode, DecodeError, FrameBuffer};

pub struct OpusDecoder {
    opus: opus::Decoder,
}

impl OpusDecoder {
    pub fn new() -> Result<Self, opus::Error> {
        let opus = opus::Decoder::new(
            SAMPLE_RATE.0,
            opus::Channels::Stereo,
        )?;

        Ok(OpusDecoder { opus })
    }
}

impl Display for OpusDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opus")
    }
}

impl Decode for OpusDecoder {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer) -> Result<(), DecodeError> {
        let expected = out.len();

        let frames = match bytes {
            Some(bytes) => self.opus.decode_float(bytes, audio::as_interleaved_mut(out), false)?,
            None => self.opus.decode_float(&[], audio::as_interleaved_mut(out), true)?,
        };

        if expected != frames {
            return Err(DecodeError::WrongFrameCount { frames, expected });
        }

        Ok(())
    }
}
