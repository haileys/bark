pub mod format;

use bytemuck::{Pod, Zeroable};

pub trait SampleFormat: Pod + Zeroable + soxr::format::Sample {
    type Frame: Pod + Zeroable;

    // the following two functions allow for runtime dispatch according to
    // buffer type
    fn sample_buffer(buffer: &[Self::Frame]) -> SampleBuffer<'_>;
    fn sample_buffer_mut(buffer: &mut [Self::Frame]) -> SampleBufferMut<'_>;
}

/// Interleaved stereo sample buffer ref
pub enum SampleBuffer<'a> {
    S16(&'a [i16]),
    F32(&'a [f32]),
}

/// Interleaved stereo sample buffer mut ref
pub enum SampleBufferMut<'a> {
    S16(&'a mut [i16]),
    F32(&'a mut [f32]),
}

impl SampleFormat for i16 {
    type Frame = FrameS16;

    fn sample_buffer(buffer: &[Self::Frame]) -> SampleBuffer<'_> {
        SampleBuffer::S16(as_interleaved(buffer))
    }

    fn sample_buffer_mut(buffer: &mut [Self::Frame]) -> SampleBufferMut<'_> {
        SampleBufferMut::S16(as_interleaved_mut(buffer))
    }
}

impl SampleFormat for f32 {
    type Frame = FrameF32;

    fn sample_buffer(buffer: &[Self::Frame]) -> SampleBuffer<'_> {
        SampleBuffer::F32(as_interleaved(buffer))
    }

    fn sample_buffer_mut(buffer: &mut [Self::Frame]) -> SampleBufferMut<'_> {
        SampleBufferMut::F32(as_interleaved_mut(buffer))
    }
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
