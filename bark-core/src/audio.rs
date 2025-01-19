use bytemuck::{Pod, Zeroable};

pub trait Format: Send + Sync + 'static {
    type Frame: Pod + Zeroable + Copy + Clone + Send;
    type Sample: Pod + Zeroable + Copy + Clone + Send + soxr::format::Sample;
    const KIND: FormatKind;

    fn frames(frames: &[Self::Frame]) -> Frames;
    fn frames_mut(frames: &mut [Self::Frame]) -> FramesMut;
}

pub enum FormatKind {
    S16,
    F32,
}

pub struct S16;
impl Format for S16 {
    type Frame = FrameS16;
    type Sample = i16;
    const KIND: FormatKind = FormatKind::S16;

    fn frames(frames: &[Self::Frame]) -> Frames {
        Frames::S16(frames)
    }

    fn frames_mut(frames: &mut [Self::Frame]) -> FramesMut {
        FramesMut::S16(frames)
    }
}

pub struct F32;
impl Format for F32 {
    type Frame = FrameF32;
    type Sample = f32;
    const KIND: FormatKind = FormatKind::F32;

    fn frames(frames: &[Self::Frame]) -> Frames {
        Frames::F32(frames)
    }

    fn frames_mut(frames: &mut [Self::Frame]) -> FramesMut {
        FramesMut::F32(frames)
    }
}

#[derive(Debug)]
pub enum Frames<'a> {
    S16(&'a [FrameS16]),
    F32(&'a [FrameF32]),
}

#[derive(Debug)]
pub enum FramesMut<'a> {
    S16(&'a mut [FrameS16]),
    F32(&'a mut [FrameF32]),
}

impl<'a> Frames<'a> {
    pub fn len(&self) -> usize {
        match self {
            Frames::S16(f) => f.len(),
            Frames::F32(f) => f.len(),
        }
    }
}

impl<'a> FramesMut<'a> {
    pub fn len(&self) -> usize {
        match self {
            FramesMut::S16(f) => f.len(),
            FramesMut::F32(f) => f.len(),
        }
    }
}

#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C)]
pub struct FrameF32(pub f32, pub f32);

#[derive(Pod, Zeroable, Copy, Clone, Debug)]
#[repr(C)]
pub struct FrameS16(pub i16, pub i16);

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct FrameCount(pub usize);

pub fn as_interleaved<F: Format>(frames: &[F::Frame]) -> &[F::Sample] {
    bytemuck::must_cast_slice(frames)
}

pub fn as_interleaved_mut<F: Format>(frames: &mut [F::Frame]) -> &mut [F::Sample] {
    bytemuck::must_cast_slice_mut(frames)
}

pub fn s16_to_f32(input: i16) -> f32 {
    let scale = i16::MIN as f32;
    input as f32 / -scale
}

pub fn f32_to_s16(input: f32) -> i16 {
    let scale = i16::MIN as f32;
    let output = (input * -scale).clamp(i16::MIN as f32, i16::MAX as f32);
    output as i16
}
