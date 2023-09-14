#![no_std]
extern crate alloc;

pub mod time;
pub mod types;
pub mod packet;

pub const SAMPLE_RATE: SampleRate = SampleRate(48000);
pub const CHANNELS: ChannelCount = ChannelCount(2);
pub const FRAMES_PER_PACKET: usize = 160;
pub const SAMPLES_PER_PACKET: usize = CHANNELS.0 as usize * FRAMES_PER_PACKET;

#[derive(Copy, Clone, Debug)]
pub struct SampleRate(pub u32);

#[derive(Copy, Clone, Debug)]
pub struct ChannelCount(pub u16);

impl From<SampleRate> for usize {
    fn from(value: SampleRate) -> Self {
        value.0.try_into().expect("SampleRate -> usize")
    }
}

impl From<SampleRate> for u32 {
    fn from(value: SampleRate) -> Self {
        value.0.into()
    }
}

impl From<SampleRate> for u64 {
    fn from(value: SampleRate) -> Self {
        value.0.into()
    }
}

impl From<SampleRate> for u128 {
    fn from(value: SampleRate) -> Self {
        value.0.into()
    }
}

impl From<SampleRate> for i64 {
    fn from(value: SampleRate) -> Self {
        value.0.into()
    }
}

impl From<ChannelCount> for usize {
    fn from(value: ChannelCount) -> Self {
        value.0.into()
    }
}

impl From<ChannelCount> for u64 {
    fn from(value: ChannelCount) -> Self {
        value.0.into()
    }
}

impl From<ChannelCount> for u32 {
    fn from(value: ChannelCount) -> Self {
        value.0.into()
    }
}
