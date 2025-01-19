use core::fmt::{self, Display};

use bark_protocol::types::AudioPacketFormat;

use crate::audio::{self, f32_to_s16, s16_to_f32, Format, Frames, F32, S16};

use super::{Encode, EncodeError};

pub struct S16LEEncoder;

impl Display for S16LEEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "signed16 (little endian)")
    }
}

impl Encode for S16LEEncoder {
    fn header_format(&self) -> AudioPacketFormat {
        AudioPacketFormat::S16LE
    }

    fn encode_packet(&mut self, frames: Frames, out: &mut [u8]) -> Result<usize, EncodeError> {
        encode_packed(frames, out, encode_i16_to_s16le, encode_f32_to_s16le)
    }
}

fn encode_i16_to_s16le(sample: i16) -> [u8; 2] {
    i16::to_le_bytes(sample)
}

fn encode_f32_to_s16le(sample: f32) -> [u8; 2] {
    i16::to_le_bytes(f32_to_s16(sample))
}

pub struct F32LEEncoder;

impl Display for F32LEEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "float32 (little endian)")
    }
}

impl Encode for F32LEEncoder {
    fn header_format(&self) -> AudioPacketFormat {
        AudioPacketFormat::F32LE
    }

    fn encode_packet(&mut self, frames: Frames, out: &mut [u8]) -> Result<usize, EncodeError> {
        encode_packed(frames, out, encode_i16_to_f32le, encode_f32_to_f32le)
    }
}

fn encode_i16_to_f32le(sample: i16) -> [u8; 4] {
    f32::to_le_bytes(s16_to_f32(sample))
}

fn encode_f32_to_f32le(sample: f32) -> [u8; 4] {
    f32::to_le_bytes(sample)
}

fn encode_packed<const N: usize>(
    frames: Frames,
    out: &mut [u8],
    encode_s16: impl Fn(i16) -> [u8; N],
    encode_f32: impl Fn(f32) -> [u8; N],
) -> Result<usize, EncodeError> {
    match frames {
        Frames::S16(frames) => encode_packed_impl::<S16, N>(frames, out, encode_s16),
        Frames::F32(frames) => encode_packed_impl::<F32, N>(frames, out, encode_f32),
    }
}

fn encode_packed_impl<F: Format, const N: usize>(
    frames: &[F::Frame],
    out: &mut [u8],
    func: impl Fn(F::Sample) -> [u8; N],
) -> Result<usize, EncodeError> {
    let samples = audio::as_interleaved::<F>(frames);
    let out = check_length(out, samples.len() * N)?;

    for (output, input) in out.chunks_exact_mut(N).zip(samples) {
        let bytes = func(*input);
        output.copy_from_slice(&bytes);
    }

    Ok(out.len())
}

fn check_length(out: &mut [u8], need: usize) -> Result<&mut [u8], EncodeError> {
    if out.len() >= need {
        Ok(&mut out[0..need])
    } else {
        Err(EncodeError::OutputBufferTooSmall { need })
    }
}
