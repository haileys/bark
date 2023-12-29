use bark_protocol::CHANNELS;
use bytemuck::{Pod, Zeroable};

pub type Sample = f32;

#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C)]
pub struct Frame(pub Sample, pub Sample);

pub fn from_interleaved(samples: &[Sample]) -> &[Frame] {
    // ensure samples contains whole frames only
    assert_eq!(0, samples.len() % usize::from(CHANNELS));

    bytemuck::cast_slice(samples)
}

pub fn from_interleaved_mut(samples: &mut [Sample]) -> &mut [Frame] {
    // ensure samples contains whole frames only
    assert_eq!(0, samples.len() % usize::from(CHANNELS));

    bytemuck::cast_slice_mut(samples)
}

pub fn to_interleaved(frames: &[Frame]) -> &[Sample] {
    bytemuck::must_cast_slice(frames)
}

pub fn to_interleaved_mut(frames: &mut [Frame]) -> &mut [Sample] {
    bytemuck::must_cast_slice_mut(frames)
}
