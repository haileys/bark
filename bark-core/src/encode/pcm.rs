use core::fmt::{self, Display};

use bark_protocol::types::AudioPacketFormat;

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

    fn encode_packet(&mut self, samples: &[f32], out: &mut [u8]) -> Result<usize, EncodeError> {
        encode_packed(samples, out, |sample| {
            let scale = (u16::MIN as f32).abs();
            let sample = sample.clamp(-1.0, 1.0) * scale;
            i16::to_le_bytes(sample as i16)
        })
    }
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

    fn encode_packet(&mut self, samples: &[f32], out: &mut [u8]) -> Result<usize, EncodeError> {
        encode_packed(samples, out, f32::to_le_bytes)
    }
}

fn encode_packed<const N: usize>(
    samples: &[f32],
    out: &mut [u8],
    func: impl Fn(f32) -> [u8; N],
) -> Result<usize, EncodeError> {
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
