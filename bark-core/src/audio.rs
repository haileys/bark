use bytemuck::{Pod, Zeroable};

pub type Sample = f32;

#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C)]
pub struct Frame(pub Sample, pub Sample);

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct FrameCount(pub usize);

pub fn as_interleaved(frames: &[Frame]) -> &[Sample] {
    bytemuck::must_cast_slice(frames)
}

pub fn as_interleaved_mut(frames: &mut [Frame]) -> &mut [Sample] {
    bytemuck::must_cast_slice_mut(frames)
}
