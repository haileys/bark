#![no_std]

use derive_more::Into;

pub mod buffer;
pub mod packet;
pub mod time;
pub mod types;

pub const SAMPLE_RATE: SampleRate = SampleRate(48000);
pub const CHANNELS: ChannelCount = ChannelCount(2);
pub const FRAMES_PER_PACKET: usize = 120; // 2.5ms at 48khz, compatible with opus
pub const SAMPLES_PER_PACKET: usize = CHANNELS.0 as usize * FRAMES_PER_PACKET;

#[derive(Copy, Clone, Debug, Into)]
#[into(u64, u128, i64, f64)]
pub struct SampleRate(pub u32);

#[derive(Copy, Clone, Debug, Into)]
#[into(usize, u32, u64)]
pub struct ChannelCount(pub u16);

impl From<SampleRate> for usize {
    fn from(value: SampleRate) -> Self {
        value.0.try_into().expect("SampleRate -> usize")
    }
}
