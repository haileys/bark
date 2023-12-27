use bytemuck::{Pod, Zeroable};

pub mod stats;

use crate::SAMPLES_PER_PACKET;

#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, Eq)]
#[repr(transparent)]
pub struct Magic(u32);

impl Magic {
    pub const AUDIO: Magic       = Magic(0x00a79ae2);
    pub const TIME: Magic        = Magic(0x01a79ae2);
    pub const STATS_REQ: Magic   = Magic(0x02a79ae2);
    pub const STATS_REPLY: Magic = Magic(0x03a79ae2);
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
    // detect new stream starts, used by senders to detect stream takeovers
    pub sid: SessionId,

    // packet sequence number - monotonic + gapless, arbitrary start point
    pub seq: u64,

    // presentation timestamp - used by receivers to detect + correct clock
    // drift
    pub pts: TimestampMicros,

    // data timestamp - the stream's clock when packet is sent
    pub dts: TimestampMicros,

    pub format: AudioPacketFormat,
}

/// This, regrettably, has to be a u64 to fill out `AudioPacketHeader` with
/// no hidden padding. TODO this whole protocol tier needs a big rethink
#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, Eq)]
#[repr(transparent)]
pub struct AudioPacketFormat(u64);

impl AudioPacketFormat {
    pub const F32LE: Self = Self(1);
    pub const S16LE: Self = Self(2);
    pub const OPUS: Self = Self(3);
}

pub type AudioPacketBuffer = [f32; SAMPLES_PER_PACKET];

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct TimePacket {
    pub sid: SessionId,
    pub rid: ReceiverId,

    pub stream_1: TimestampMicros,
    pub receive_2: TimestampMicros,
    pub stream_3: TimestampMicros,
}

#[derive(Debug, PartialEq)]
pub enum TimePhase {
    /// The initial phase, the stream server sends out a broadcast time packet
    /// withn only `stream_1` set
    Broadcast,

    /// A receiver replies, setting `receive_2`
    ReceiverReply,

    /// Finally, the stream replies (over unicast) again, setting `stream_3`
    StreamReply,
}

impl TimePacket {
    pub fn phase(&self) -> Option<TimePhase> {
        let t1 = self.stream_1.0;
        let t2 = self.receive_2.0;
        let t3 = self.stream_3.0;

        if t1 != 0 && t2 == 0 && t3 == 0 {
            return Some(TimePhase::Broadcast);
        }

        if t1 != 0 && t2 != 0 && t3 == 0 {
            return Some(TimePhase::ReceiverReply);
        }

        if t1 != 0 && t2 != 0 && t3 != 0 {
            return Some(TimePhase::StreamReply);
        }

        // incoherent + invalid time packet
        None
    }
}

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

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(transparent)]
pub struct TimestampMicros(pub u64);

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
