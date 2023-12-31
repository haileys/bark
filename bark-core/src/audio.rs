use bytemuck::{Pod, Zeroable};

pub unsafe trait SampleFormat: Pod {
    type Frame: Pod;
    const FORMAT: SampleFormatEnum;
}

pub enum SampleFormatEnum {
    S16,
    F32,
}

unsafe impl SampleFormat for i16 {
    type Frame = FrameS16;
    const FORMAT: SampleFormatEnum = SampleFormatEnum::S16;
}

unsafe impl SampleFormat for f32 {
    type Frame = FrameF32;
    const FORMAT: SampleFormatEnum = SampleFormatEnum::F32;
}

pub type Sample = f32;
pub type Frame = <Sample as SampleFormat>::Frame;

#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C)]
pub struct FrameS16(pub i16, pub i16);

#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C)]
pub struct FrameF32(pub f32, pub f32);

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct FrameCount(pub usize);

pub fn as_interleaved<S: SampleFormat>(frames: &[S::Frame]) -> &[S] {
    bytemuck::must_cast_slice(frames)
}

pub fn as_interleaved_mut<S: SampleFormat>(frames: &mut [S::Frame]) -> &mut [S] {
    bytemuck::must_cast_slice_mut(frames)
}
