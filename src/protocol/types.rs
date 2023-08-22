use bytemuck::{Pod, Zeroable};
use nix::time::ClockId;
use nix::sys::time::TimeValLike;

use crate::stats;
use crate::protocol;

pub const MAGIC_AUDIO: u32       = 0x00a79ae2;
pub const MAGIC_TIME: u32        = 0x01a79ae2;
pub const MAGIC_STATS_REQ: u32   = 0x02a79ae2;
pub const MAGIC_STATS_REPLY: u32 = 0x03a79ae2;

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
    pub sid: SessionId,

    // packet sequence number - monotonic + gapless, arbitrary start point
    pub seq: u64,

    // presentation timestamp - used by receivers to detect + correct clock
    // drift
    pub pts: TimestampMicros,

    // data timestamp - the stream's clock when packet is sent
    pub dts: TimestampMicros,

    // audio data:
    pub buffer: PacketBuffer,
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct TimePacket {
    pub magic: u32,
    pub flags: u32,
    pub sid: SessionId,
    pub rid: ReceiverId,

    pub stream_1: TimestampMicros,
    pub receive_2: TimestampMicros,
    pub stream_3: TimestampMicros,

    // packet delay has a linear relationship to packet size - it's important
    // that time packets experience as similar delay as possible to audio
    // packets for most accurate synchronisation, so we add some padding here
    pub _pad: TimePacketPadding,
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
pub struct StatsRequestPacket {
    pub magic: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct StatsReplyPacket {
    pub magic: u32,
    pub flags: StatsReplyFlags,

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

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PacketBuffer(pub [f32; protocol::SAMPLES_PER_PACKET]);

/// SAFETY: Pod is impl'd for f32, and [T: Pod; N: usize]
/// but for some reason doesn't like N == SAMPLES_PER_PACKET?
unsafe impl Pod for PacketBuffer {}

/// SAFETY: Zeroable is impl'd for f32, and [T: Zeroable; N: usize]
/// but for some reason doesn't like N == SAMPLES_PER_PACKET?
unsafe impl Zeroable for PacketBuffer {
    fn zeroed() -> Self {
        PacketBuffer([0f32; protocol::SAMPLES_PER_PACKET])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimePacketPadding([u8; 1272]);

// SAFETY: same as above in PacketBuffer
unsafe impl Pod for TimePacketPadding {}

// SAFETY: same as above in PacketBuffer
unsafe impl Zeroable for TimePacketPadding {
    fn zeroed() -> Self {
        TimePacketPadding([0u8; 1272])
    }
}

// assert that AudioPacket and TimePacket are the same size, see comment for
// TimePacket::_pad field
static_assertions::assert_eq_size!(AudioPacket, TimePacket);

#[repr(C)]
pub union PacketUnion {
    _1: AudioPacket,
    _2: TimePacket,
    _3: StatsRequestPacket,
    _4: StatsReplyPacket,
}

pub enum Packet<'a> {
    Audio(&'a mut AudioPacket),
    Time(&'a mut TimePacket),
    StatsRequest(&'a mut StatsRequestPacket),
    StatsReply(&'a mut StatsReplyPacket),
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

        if magic == MAGIC_STATS_REQ {
            return Some(Packet::StatsRequest(bytemuck::try_from_bytes_mut(raw).ok()?));
        }

        if magic == MAGIC_STATS_REPLY {
            return Some(Packet::StatsReply(bytemuck::try_from_bytes_mut(raw).ok()?));
        }

        None
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(transparent)]
pub struct TimestampMicros(pub u64);

impl TimestampMicros {
    pub fn now() -> TimestampMicros {
        let timespec = nix::time::clock_gettime(ClockId::CLOCK_BOOTTIME)
            .expect("clock_gettime(CLOCK_BOOTTIME) failed, are we on Linux?");

        let micros = u64::try_from(timespec.num_microseconds())
            .expect("cannot convert i64 time value to u64");

        TimestampMicros(micros)
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(transparent)]
pub struct ReceiverId(u64);

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

    pub fn generate() -> Self {
        ReceiverId(rand::random())
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct SessionId(i64);

impl SessionId {
    pub fn generate() -> Self {
        let timespec = nix::time::clock_gettime(ClockId::CLOCK_REALTIME)
            .expect("clock_gettime(CLOCK_REALTIME)");

        SessionId(timespec.num_microseconds())
    }
}
