use core::fmt::{self, Display};

use crate::audio;

use super::{Decode, DecodeError, FrameBuffer};

pub struct S16LEDecoder;

impl Display for S16LEDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "signed16 (little endian)")
    }
}

impl Decode for S16LEDecoder {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer) -> Result<(), DecodeError> {
        decode_packed(bytes, out, |bytes| {
            let input = i16::from_le_bytes(bytes);
            let scale = i16::MAX as f32;
            input as f32 / scale
        })
    }
}

pub struct F32LEDecoder;

impl Display for F32LEDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "float32 (little endian)")
    }
}

impl Decode for F32LEDecoder {
    fn decode_packet(&mut self, bytes: Option<&[u8]>, out: &mut FrameBuffer) -> Result<(), DecodeError> {
        decode_packed(bytes, out, f32::from_le_bytes)
    }
}

fn decode_packed<const N: usize>(
    bytes: Option<&[u8]>,
    out: &mut FrameBuffer,
    func: impl Fn([u8; N]) -> f32,
) -> Result<(), DecodeError> {
    let out_samples = audio::as_interleaved_mut(out);

    let Some(bytes) = bytes else {
        // PCM codecs have no packet loss correction
        // just zero fill and return
        out_samples.fill(0.0);
        return Ok(());
    };

    check_length(bytes, out_samples.len() * N)?;

    for (input, output) in bytes.chunks_exact(N).zip(out_samples) {
        // when array_chunks stabilises we can use that instead
        // but for now use try_into to turn a &[u8] (guaranteed len == width)
        // into a [u8; width]
        let input = input.try_into().unwrap();
        *output = func(input);
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
