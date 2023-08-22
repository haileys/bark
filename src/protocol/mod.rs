pub mod types;

pub use cpal::{SampleFormat, SampleRate, ChannelCount};

pub const SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;
pub const SAMPLE_RATE: SampleRate = SampleRate(48000);
pub const CHANNELS: ChannelCount = 2;
pub const FRAMES_PER_PACKET: usize = 160;
pub const SAMPLES_PER_PACKET: usize = CHANNELS as usize * FRAMES_PER_PACKET;

pub const MAX_PACKET_SIZE: usize = ::std::mem::size_of::<types::PacketUnion>();
