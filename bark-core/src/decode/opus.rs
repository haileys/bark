use core::fmt::{self, Display};
use core::marker::PhantomData;

use bark_protocol::SAMPLE_RATE;

use crate::audio::{SampleFormat, SampleBufferMut};
use crate::decode::{Decode, DecodeError, FrameBuffer};

pub struct OpusDecoder<S> {
    opus: opus::Decoder,
    _phantom: PhantomData<S>
}

impl<S: SampleFormat> OpusDecoder<S> {
    pub fn new() -> Result<Self, opus::Error> {
        let opus = opus::Decoder::new(
            SAMPLE_RATE.0,
            opus::Channels::Stereo,
        )?;

        Ok(OpusDecoder { opus, _phantom: PhantomData })
    }
}

impl<S> Display for OpusDecoder<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opus")
    }
}

impl<S: SampleFormat> Decode<S> for OpusDecoder<S> {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer<S>) -> Result<(), DecodeError> {
        let expected = out.len();

        let frames = match bytes {
            Some(bytes) => decode_dispatch::<S>(&mut self.opus, bytes, out, false)?,
            None => decode_dispatch::<S>(&mut self.opus, &[], out, true)?,
        };

        if expected != frames {
            return Err(DecodeError::WrongFrameCount { frames, expected });
        }

        Ok(())
    }
}

fn decode_dispatch<S: SampleFormat>(
    opus: &mut opus::Decoder,
    input: &[u8],
    output: &mut [S::Frame],
    fec: bool,
) -> opus::Result<usize> {
    match S::sample_buffer_mut(output) {
        SampleBufferMut::S16(output) => opus.decode(input, output, fec),
        SampleBufferMut::F32(output) => opus.decode_float(input, output, fec),
    }
}
