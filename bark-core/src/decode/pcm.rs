use core::fmt::{self, Display};
use std::marker::PhantomData;

use bytemuck::Zeroable;

use bark_protocol::CHANNELS;

use crate::audio::{self, SampleFormat, SampleBufferMut};
use crate::decode::{Decode, DecodeError, FrameBuffer};

pub struct S16LEDecoder<Sample>(PhantomData<Sample>);

impl<S> S16LEDecoder<S> {
    pub fn new() -> Self {
        S16LEDecoder(PhantomData)
    }
}

impl<S> Display for S16LEDecoder<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "signed16 (little endian)")
    }
}

impl<Sample: SampleFormat> Decode<Sample> for S16LEDecoder<Sample> {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer<Sample>) -> Result<(), DecodeError> {
        decode_packed::<Sample, 2>(bytes, out,
            i16::from_le_bytes,
            |bytes| audio::format::i16_to_f32(i16::from_le_bytes(bytes)),
        )
    }
}

pub struct F32LEDecoder<Sample>(PhantomData<Sample>);

impl<S> F32LEDecoder<S> {
    pub fn new() -> Self {
        F32LEDecoder(PhantomData)
    }
}

impl<S> Display for F32LEDecoder<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "float32 (little endian)")
    }
}

impl<Sample: SampleFormat> Decode<Sample> for F32LEDecoder<Sample> {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer<Sample>) -> Result<(), DecodeError> {
        decode_packed::<Sample, 4>(bytes, out,
            |bytes| audio::format::f32_to_i16(f32::from_le_bytes(bytes)),
            f32::from_le_bytes
        )
    }
}

fn decode_packed<S: SampleFormat, const N: usize>(
    bytes: Option<&[u8]>,
    out: &mut FrameBuffer<S>,
    decode_s16: impl Fn([u8; N]) -> i16,
    decode_f32: impl Fn([u8; N]) -> f32,
) -> Result<(), DecodeError> {
    let Some(bytes) = bytes else {
        // PCM codecs have no packet loss correction
        // just zero fill and return
        out.fill(S::Frame::zeroed());
        return Ok(());
    };

    check_length(bytes, out.len() * usize::from(CHANNELS) * N)?;

    let input = bytes.chunks_exact(N);

    match S::sample_buffer_mut(out) {
        SampleBufferMut::S16(out) => {
            for (input, output) in input.zip(out) {
                // when array_chunks stabilises we can use that instead
                // but for now use try_into to turn a &[u8] (guaranteed len == width)
                // into a [u8; width]
                let input = input.try_into().unwrap();
                *output = decode_s16(input);
            }
        }
        SampleBufferMut::F32(out) => {
            for (input, output) in input.zip(out) {
                // when array_chunks stabilises we can use that instead
                // but for now use try_into to turn a &[u8] (guaranteed len == width)
                // into a [u8; width]
                let input = input.try_into().unwrap();
                *output = decode_f32(input);
            }
        }
    }

    Ok(())
}

fn check_length(bytes: &[u8], expected: usize) -> Result<(), DecodeError> {
    let length = bytes.len();

    if length == expected {
        Ok(())
    } else {
        Err(DecodeError::WrongLength { length, expected })
    }
}
