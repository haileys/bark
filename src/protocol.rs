use std::time::SystemTime;

use bytemuck::{Pod, Zeroable};
use cpal::{SampleFormat, SampleRate, ChannelCount};

pub const SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;
pub const SAMPLE_RATE: SampleRate = SampleRate(48000);
pub const CHANNELS: ChannelCount = 2;
pub const FRAMES_PER_PACKET: usize = 160;
pub const SAMPLES_PER_PACKET: usize = CHANNELS as usize * FRAMES_PER_PACKET;

pub const MAGIC_AUDIO: u32 = 0x00a79ae2;
pub const MAGIC_TIME: u32  = 0x01a79ae2;

/// our network Packet struct
/// we don't need to worry about endianness, because according to the rust docs:
///
///     Floats and Ints have the same endianness on all supported platforms.
///     IEEE 754 very precisely specifies the bit layout of floats.
///
///     - https://doc.rust-lang.org/std/primitive.f32.html
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct AudioPacket {
    // magic and flags. magic is always MAGIC_AUDIO and indicates that this
    // is an audio packet. flags is always 0 for now.
    pub magic: u32,
    pub flags: u32,

    // stream id - set to the start time of a stream, used by receivers to
    // detect new stream starts, used by senders to detect stream takeovers
    pub sid: TimestampMicros,

    // packet sequence number - monotonic + gapless, arbitrary start point
    pub seq: u64,

    // presentation timestamp - used by receivers to detect + correct clock
    // drift
    pub pts: TimestampMicros,

    // audio data:
    pub buffer: PacketBuffer,
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct TimePacket {
    pub magic: u32,
    pub flags: u32,
    pub sid: TimestampMicros,
    pub t1: TimestampMicros,
    pub t2: TimestampMicros,
    pub t3: TimestampMicros,
}

pub const MAX_PACKET_SIZE: usize = ::std::mem::size_of::<PacketUnion>();

pub enum Packet<'a> {
    Audio(&'a mut AudioPacket),
    Time(&'a mut TimePacket),
}

impl<'a> Packet<'a> {
    pub fn try_from_bytes_mut(raw: &'a mut [u8]) -> Option<Packet<'a>> {
        let magic: u32 = *bytemuck::try_from_bytes(&raw[0..4]).ok()?;

        if magic == MAGIC_TIME {
            return Some(Packet::Time(bytemuck::try_from_bytes_mut(raw).ok()?));
        }

        if magic == MAGIC_AUDIO {
            return Some(Packet::Audio(bytemuck::try_from_bytes_mut(raw).ok()?));
        }

        None
    }
}

#[repr(C)]
union PacketUnion {
    audio: AudioPacket,
    time: TimePacket,
}

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PacketBuffer(pub [f32; SAMPLES_PER_PACKET]);

/// SAFETY: Pod is impl'd for f32, and [T: Pod; N: usize]
/// but for some reason doesn't like N == SAMPLES_PER_PACKET?
unsafe impl Pod for PacketBuffer {}

/// SAFETY: Zeroable is impl'd for f32, and [T: Zeroable; N: usize]
/// but for some reason doesn't like N == SAMPLES_PER_PACKET?
unsafe impl Zeroable for PacketBuffer {
    fn zeroed() -> Self {
        PacketBuffer([0f32; SAMPLES_PER_PACKET])
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(transparent)]
pub struct TimestampMicros(pub u64);

impl TimestampMicros {
    pub fn now() -> TimestampMicros {
        // SystemTime::now uses CLOCK_REALTIME on Linux, which is exactly what we want
        // https://doc.rust-lang.org/std/time/struct.SystemTime.html#platform-specific-behavior
        let micros = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime::now before UNIX_EPOCH!")
            .as_micros();

        let micros = u64::try_from(micros)
            .expect("can't narrow timestamp to u64");

        TimestampMicros(micros)
    }
}
