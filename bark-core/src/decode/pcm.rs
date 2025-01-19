use core::fmt::{self, Display};

use bytemuck::Zeroable;

use crate::audio::{self, f32_to_s16, s16_to_f32, Format, FramesMut, F32, S16};
use super::{Decode, DecodeError};

pub struct S16LEDecoder;

impl Display for S16LEDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "signed16 (little endian)")
    }
}

impl Decode for S16LEDecoder {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: FramesMut) -> Result<(), DecodeError> {
        decode_packed(bytes, out, decode_s16le_to_i16, decode_s16le_to_f32)
    }
}

fn decode_s16le_to_i16(bytes: [u8; 2]) -> i16 {
    i16::from_le_bytes(bytes)
}

fn decode_s16le_to_f32(bytes: [u8; 2]) -> f32 {
    s16_to_f32(i16::from_le_bytes(bytes))
}

pub struct F32LEDecoder;

impl Display for F32LEDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "float32 (little endian)")
    }
}

impl Decode for F32LEDecoder {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: FramesMut) -> Result<(), DecodeError> {
        decode_packed(bytes, out, decode_f32le_to_i16, decode_f32le_to_f32)
    }
}

fn decode_f32le_to_i16(bytes: [u8; 4]) -> i16 {
    let input = f32::from_le_bytes(bytes);
    f32_to_s16(input)
}

fn decode_f32le_to_f32(bytes: [u8; 4]) -> f32 {
    f32::from_le_bytes(bytes)
}

fn decode_packed<const N: usize>(
    bytes: Option<&[u8]>,
    out: FramesMut,
    decode_s16: impl Fn([u8; N]) -> i16,
    decode_f32: impl Fn([u8; N]) -> f32,
) -> Result<(), DecodeError> {
    match out {
        FramesMut::S16(out) => decode_packed_impl::<S16, N>(bytes, out, decode_s16),
        FramesMut::F32(out) => decode_packed_impl::<F32, N>(bytes, out, decode_f32),
    }
}

fn decode_packed_impl<F: Format, const N: usize>(
    bytes: Option<&[u8]>,
    out: &mut [F::Frame],
    decode: impl Fn([u8; N]) -> F::Sample,
) -> Result<(), DecodeError> {
    let out_samples = audio::as_interleaved_mut::<F>(out);

    let Some(bytes) = bytes else {
        // PCM codecs have no packet loss correction
        // just zero fill and return
        out_samples.fill(F::Sample::zeroed());
        return Ok(());
    };

    check_length(bytes, out_samples.len() * N)?;

    for (input, output) in bytes.chunks_exact(N).zip(out_samples) {
        // when array_chunks stabilises we can use that instead
        // but for now use try_into to turn a &[u8] (guaranteed len == width)
        // into a [u8; width]
        let input = input.try_into().unwrap();
        *output = decode(input);
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
