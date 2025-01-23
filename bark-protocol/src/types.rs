use core::time::Duration;

use bytemuck::{Pod, Zeroable};

pub mod stats;

use crate::SAMPLES_PER_PACKET;

#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, Eq)]
#[repr(transparent)]
pub struct Magic(u32);

impl Magic {
    const fn tag(tag: u8) -> Self {
        Magic(((tag as u32) << 24) | 0x00a79ae2)
    }

    pub const AUDIO: Magic       = Magic::tag(0x00);
    pub const STATS_REQ: Magic   = Magic::tag(0x02);
    pub const STATS_REPLY: Magic = Magic::tag(0x03);
    pub const PING: Magic        = Magic::tag(0x04);
    pub const PONG: Magic        = Magic::tag(0x05);
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct PacketHeader {
    // magic and flags. there is a distinct magic value for each packet type,
    // and flags has a packet-dependent meaning.
    pub magic: Magic,
    pub flags: u32,
}

/// our network Packet struct
/// we don't need to worry about endianness, because according to the rust docs:
///
///     Floats and Ints have the same endianness on all supported platforms.
///     IEEE 754 very precisely specifies the bit layout of floats.
///
///     - https://doc.rust-lang.org/std/primitive.f32.html
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct AudioPacketHeader {
    // stream id - set to the start time of a stream, used by receivers to
    // detect newer streams in same priority rank
    pub sid: SessionId,

    // packet sequence number - monotonic + gapless, arbitrary start point
    pub seq: u64,

    // presentation timestamp
    pub pts: TimestampMicros,

    // data timestamp
    pub dts: TimestampMicros,

    pub format: AudioPacketFormat,
    pub priority: i8,

    pub padding: [u8; 6],
}

#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, Eq)]
#[repr(transparent)]
pub struct AudioPacketFormat(u8);

impl AudioPacketFormat {
    pub const F32LE: Self = Self(1);
    pub const S16LE: Self = Self(2);
    pub const OPUS: Self = Self(3);
}

pub type AudioPacketBuffer = [f32; SAMPLES_PER_PACKET];

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct StatsReplyPacket {
    pub sid: SessionId,
    pub receiver: stats::receiver::ReceiverStats,
    pub node: stats::node::NodeStats,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Zeroable, Pod)]
    #[repr(transparent)]
    pub struct StatsReplyFlags: u32 {
        const IS_RECEIVER = 0x01;
        const IS_STREAM   = 0x02;
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct TimestampMicros(pub u64);

impl TimestampMicros {
    pub fn saturating_sub(&self, duration: Duration) -> TimestampMicros {
        let duration = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        TimestampMicros(self.0.saturating_sub(duration))
    }

    pub fn saturating_duration_since(&self, rhs: TimestampMicros) -> Duration {
        let micros = self.0.saturating_sub(rhs.0);
        Duration::from_micros(micros)
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(transparent)]
pub struct ReceiverId(pub u64);

impl ReceiverId {
    pub fn broadcast() -> Self {
        ReceiverId(0)
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == 0
    }

    pub fn matches(&self, this: &ReceiverId) -> bool {
        self.is_broadcast() || self.0 == this.0
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct SessionId(pub i64);
