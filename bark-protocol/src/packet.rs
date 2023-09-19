use core::cmp;
use core::mem::size_of;
use core::ops::Range;

use bytemuck::Zeroable;

use crate::buffer::{AllocError, PacketBuffer};
use crate::types::stats::node::NodeStats;
use crate::types::stats::receiver::ReceiverStats;
use crate::types::{self, AudioPacketHeader, Magic, SessionId, StatsReplyFlags};
use crate::time::SampleDuration;

pub const MAX_PACKET_SIZE: usize =
    size_of::<types::PacketHeader>() +
    size_of::<types::AudioPacketHeader>() +
    size_of::<types::AudioPacketBuffer>();

#[derive(Debug)]
pub struct Packet(PacketBuffer);

impl Packet {
    fn allocate(magic: Magic, len: usize) -> Result<Self, AllocError> {
        let header_size = size_of::<types::PacketHeader>();
        let packet_len = header_size + len;

        let mut packet = Packet(PacketBuffer::allocate(packet_len)?);
        packet.header_mut().magic = magic;
        Ok(packet)
    }

    pub fn from_buffer(buffer: PacketBuffer) -> Option<Packet> {
        let header_size = size_of::<types::PacketHeader>();
        if buffer.len() < header_size {
            None
        } else {
            Some(Packet(buffer))
        }
    }

    pub fn as_buffer(&self) -> &PacketBuffer {
        &self.0
    }

    pub fn parse(self) -> Option<PacketKind> {
        match self.header().magic {
            Magic::AUDIO => Audio::parse(self).map(PacketKind::Audio),
            Magic::TIME => Time::parse(self).map(PacketKind::Time),
            Magic::STATS_REQ => StatsRequest::parse(self).map(PacketKind::StatsRequest),
            Magic::STATS_REPLY => StatsReply::parse(self).map(PacketKind::StatsReply),
            _ => None,
        }
    }

    pub fn header(&self) -> &types::PacketHeader {
        let header_size = size_of::<types::PacketHeader>();
        let header_bytes = &self.0.as_bytes()[0..header_size];
        bytemuck::from_bytes(header_bytes)
    }

    pub fn header_mut(&mut self) -> &mut types::PacketHeader {
        let header_size = size_of::<types::PacketHeader>();
        let header_bytes = &mut self.0.as_bytes_mut()[0..header_size];
        bytemuck::from_bytes_mut(header_bytes)
    }

    pub fn len(&self) -> usize {
        let header_size = size_of::<types::PacketHeader>();
        self.0.len() - header_size
    }

    pub fn as_bytes(&self) -> &[u8] {
        let header_size = size_of::<types::PacketHeader>();
        &self.0.as_bytes()[header_size..]
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        let header_size = size_of::<types::PacketHeader>();
        &mut self.0.as_bytes_mut()[header_size..]
    }
}

#[derive(Debug)]
pub enum PacketKind {
    Audio(Audio),
    Time(Time),
    StatsRequest(StatsRequest),
    StatsReply(StatsReply),
}

#[derive(Debug)]
pub struct Audio(Packet);

impl Audio {
    const LENGTH: usize =
        size_of::<types::AudioPacketHeader>() +
        size_of::<types::AudioPacketBuffer>();

    pub fn write() -> Result<AudioWriter, AllocError> {
        let packet = Packet::allocate(Magic::AUDIO, Self::LENGTH)?;

        Ok(AudioWriter {
            packet: Audio(packet),
            written: SampleDuration::zero(),
        })
    }

    pub fn parse(packet: Packet) -> Option<Self> {
        if packet.len() != Self::LENGTH {
            return None;
        }

        if packet.header().flags != 0 {
            return None;
        }

        Some(Audio(packet))
    }

    pub fn as_packet(&self) -> &Packet {
        &self.0
    }

    pub fn buffer(&self) -> &[f32] {
        let header_size = size_of::<types::AudioPacketHeader>();
        let buffer_bytes = &self.0.as_bytes()[header_size..];
        bytemuck::cast_slice(buffer_bytes)
    }

    pub fn buffer_mut(&mut self) -> &mut [f32] {
        let header_size = size_of::<types::AudioPacketHeader>();
        let buffer_bytes = &mut self.0.as_bytes_mut()[header_size..];
        bytemuck::cast_slice_mut(buffer_bytes)
    }

    pub fn header(&self) -> &types::AudioPacketHeader {
        let header_size = size_of::<types::AudioPacketHeader>();
        let header_bytes = &self.0.as_bytes()[0..header_size];
        bytemuck::from_bytes(header_bytes)
    }

    pub fn header_mut(&mut self) -> &mut types::AudioPacketHeader {
        let header_size = size_of::<types::AudioPacketHeader>();
        let header_bytes = &mut self.0.as_bytes_mut()[0..header_size];
        bytemuck::from_bytes_mut(header_bytes)
    }
}

#[derive(Debug)]
pub struct AudioWriter {
    packet: Audio,
    written: SampleDuration,
}

impl AudioWriter {
    pub fn length(&self) -> SampleDuration {
        self.written
    }

    pub fn remaining(&self) -> SampleDuration {
        SampleDuration::ONE_PACKET.sub(self.length())
    }

    fn remaining_buffer_mut(&mut self) -> &mut [f32] {
        let offset = self.length().as_buffer_offset();
        &mut self.packet.buffer_mut()[offset..]
    }

    pub fn valid_length(&self) -> bool {
        self.remaining() == SampleDuration::zero()
    }

    pub fn write(&mut self, audio: &[f32]) -> SampleDuration {
        let input_duration = SampleDuration::from_buffer_offset(audio.len());
        let copy_duration = cmp::min(input_duration, self.remaining());

        let copy_len = copy_duration.as_buffer_offset();
        let source_buffer = &audio[0..copy_len];
        let dest_buffer = &mut self.remaining_buffer_mut()[0..copy_len];
        dest_buffer.copy_from_slice(source_buffer);

        self.written = self.written.add(copy_duration);

        copy_duration
    }

    pub fn finalize(mut self, header: AudioPacketHeader) -> Audio {
        if !self.valid_length() {
            panic!("into_audio_packet called on writer with invalid length");
        }

        *self.packet.header_mut() = header;
        self.packet
    }
}

#[derive(Debug)]
pub struct Time(Packet);

impl Time {
    // packet delay has a linear relationship to packet size - it's important
    // that time packets experience as similar delay as possible to audio
    // packets for most accurate synchronisation, so we pad this packet out
    // to the same size as the audio packet
    const LENGTH: usize = Audio::LENGTH;

    // time packets are padded so that they are
    // the same length as audio packets:
    const DATA_RANGE: Range<usize> =
        0..size_of::<types::TimePacket>();

    pub fn allocate() -> Result<Self, AllocError> {
        Ok(Time(Packet::allocate(Magic::TIME, Self::LENGTH)?))
    }

    pub fn parse(packet: Packet) -> Option<Self> {
        // we add some padding to the time packet so that it is the same
        // length as audio packets
        if packet.len() < Self::LENGTH {
            return None;
        }

        if packet.header().flags != 0 {
            return None;
        }

        Some(Time(packet))
    }

    pub fn as_packet(&self) -> &Packet {
        &self.0
    }

    pub fn data(&self) -> &types::TimePacket {
        bytemuck::from_bytes(&self.0.as_bytes()[Self::DATA_RANGE])
    }

    pub fn data_mut(&mut self) -> &mut types::TimePacket {
        bytemuck::from_bytes_mut(&mut self.0.as_bytes_mut()[Self::DATA_RANGE])
    }
}

#[derive(Debug)]
pub struct StatsRequest(Packet);

impl StatsRequest {
    pub fn new() -> Result<Self, AllocError> {
        Ok(StatsRequest(Packet::allocate(Magic::STATS_REQ, 0)?))
    }

    pub fn parse(packet: Packet) -> Option<Self> {
        if packet.len() != 0 {
            return None;
        }

        if packet.header().flags != 0 {
            return None;
        }

        Some(StatsRequest(packet))
    }

    pub fn as_packet(&self) -> &Packet {
        &self.0
    }
}

#[derive(Debug)]
pub struct StatsReply(Packet);

impl StatsReply {
    const LENGTH: usize = size_of::<types::StatsReplyPacket>();

    fn new(flags: StatsReplyFlags, data: types::StatsReplyPacket) -> Result<Self, AllocError> {
        let mut packet = Packet::allocate(Magic::STATS_REPLY, Self::LENGTH)?;
        packet.header_mut().flags = bytemuck::cast(flags);

        let mut reply = StatsReply(packet);
        *reply.data_mut() = data;

        Ok(reply)
    }

    pub fn source(sid: SessionId, node: NodeStats) -> Result<Self, AllocError> {
        let receiver = ReceiverStats::zeroed();

        Self::new(
            StatsReplyFlags::IS_STREAM,
            types::StatsReplyPacket { sid, receiver, node },
        )
    }

    pub fn receiver(sid: SessionId, receiver: ReceiverStats, node: NodeStats) -> Result<Self, AllocError> {
        Self::new(
            StatsReplyFlags::IS_RECEIVER,
            types::StatsReplyPacket { sid, receiver, node },
        )
    }

    pub fn parse(packet: Packet) -> Option<Self> {
        if packet.len() != Self::LENGTH {
            return None;
        }

        Some(StatsReply(packet))
    }

    pub fn as_packet(&self) -> &Packet {
        &self.0
    }

    pub fn flags(&self) -> types::StatsReplyFlags {
        bytemuck::cast(self.0.header().flags)
    }

    pub fn data(&self) -> &types::StatsReplyPacket {
        bytemuck::from_bytes(self.0.as_bytes())
    }

    pub fn data_mut(&mut self) -> &mut types::StatsReplyPacket {
        bytemuck::from_bytes_mut(self.0.as_bytes_mut())
    }
}
