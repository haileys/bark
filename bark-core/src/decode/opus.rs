use core::fmt::{self, Display};
use core::marker::PhantomData;

use bark_protocol::SAMPLE_RATE;

use crate::audio::{self, SampleFormat};
use crate::decode::{Decode, DecodeError, FrameBuffer};

pub struct OpusDecoder<Sample> {
    decode: PolyDecode<Sample>,
}

impl<Sample: SampleFormat> OpusDecoder<Sample> {
    pub fn new() -> Result<Self, opus::Error> {
        let opus = opus::Decoder::new(
            SAMPLE_RATE.0,
            opus::Channels::Stereo,
        )?;

        Ok(OpusDecoder { opus, _phantom: PhantomData })
    }
}

impl<Sample: SampleFormat> Display for OpusDecoder<Sample> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opus")
    }
}

impl<Sample: SampleFormat> Decode<Sample> for OpusDecoder<Sample> {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer<f32>) -> Result<(), DecodeError> {
        let expected = out.len();

        fn decode_inner<S: SampleFormat>(
            opus: &mut opus::Decoder,
            bytes: &[u8],
            out: &mut [S::Frame],
            fec: bool,
        ) -> opus::Result<usize> {
            match S::FORMAT {

            }
        }

        let frames = match bytes {
            Some(bytes) => self.decode.decode(bytes, audio::as_interleaved_mut(out), false)?,
            None => self.decode.decode(&[], audio::as_interleaved_mut(out), true)?,
        };

        if expected != frames {
            return Err(DecodeError::WrongFrameCount { frames, expected });
        }

        Ok(())
    }
}
