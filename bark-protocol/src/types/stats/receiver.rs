use bitflags::bitflags;
use bytemuck::{Zeroable, Pod};

use crate::time::{SampleDuration, Timestamp};

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct ReceiverStats {
    flags: ReceiverStatsFlags,
    stream_status: u8,
    _pad: [u8; 6],

    audio_offset: f64,
    buffer_length: f64,
    network_latency: f64,
    predict_offset: f64,
}

pub enum StreamStatus {
    Seek,
    Sync,
    Slew,
    Miss,
}

impl StreamStatus {
    fn into_u8(&self) -> u8 {
        match self {
            StreamStatus::Seek => 1,
            StreamStatus::Sync => 2,
            StreamStatus::Slew => 3,
            StreamStatus::Miss => 4,
        }
    }

    fn from_u8(u: u8) -> Option<Self> {
        match u {
            1 => Some(StreamStatus::Seek),
            2 => Some(StreamStatus::Sync),
            3 => Some(StreamStatus::Slew),
            4 => Some(StreamStatus::Miss),
            _ => None,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, Zeroable, Pod)]
    #[repr(transparent)]
    pub struct ReceiverStatsFlags: u8 {
        const HAS_AUDIO_LATENCY   = 0x04;
        const HAS_BUFFER_LENGTH   = 0x08;
        const HAS_NETWORK_LATENCY = 0x10;
        const HAS_PREDICT_OFFSET  = 0x20;
    }
}

impl ReceiverStats {
    pub fn new() -> Self {
        ReceiverStats::zeroed()
    }

    pub fn stream(&self) -> Option<StreamStatus> {
        StreamStatus::from_u8(self.stream_status)
    }

    pub fn set_stream(&mut self, status: StreamStatus) {
        self.stream_status = status.into_u8();
    }

    pub fn clear(&mut self) {
        self.set_stream(StreamStatus::Seek);
        self.flags = ReceiverStatsFlags::empty();
    }

    fn field(&self, flag: ReceiverStatsFlags, value: f64) -> Option<f64> {
        if self.flags.contains(flag) {
            Some(value)
        } else {
            None
        }
    }

    /// Audio latency in seconds
    pub fn audio_latency(&self) -> Option<f64> {
        self.field(ReceiverStatsFlags::HAS_AUDIO_LATENCY, self.audio_offset)
    }

    /// Duration of buffered audio in seconds
    pub fn buffer_length(&self) -> Option<f64> {
        self.field(ReceiverStatsFlags::HAS_BUFFER_LENGTH, self.buffer_length)
    }

    /// Duration of buffered audio in seconds
    pub fn network_latency(&self) -> Option<f64> {
        self.field(ReceiverStatsFlags::HAS_NETWORK_LATENCY, self.network_latency)
    }

    /// Running prediction offset in seconds
    pub fn predict_offset(&self) -> Option<f64> {
        self.field(ReceiverStatsFlags::HAS_PREDICT_OFFSET, self.predict_offset)
    }

    pub fn set_audio_latency(&mut self, request_pts: Timestamp, packet_pts: Timestamp) {
        let request_micros = request_pts.to_micros_lossy().0 as f64;
        let packet_micros = packet_pts.to_micros_lossy().0 as f64;

        self.audio_offset = (request_micros - packet_micros) / 1_000_000.0;
        self.flags.insert(ReceiverStatsFlags::HAS_AUDIO_LATENCY);
    }

    pub fn set_buffer_length(&mut self, length: SampleDuration) {
        self.buffer_length = length.to_std_duration_lossy().as_micros() as f64 / 1_000_000.0;
        self.flags.insert(ReceiverStatsFlags::HAS_BUFFER_LENGTH);
    }

    pub fn set_network_latency(&mut self, latency: core::time::Duration) {
        self.network_latency = latency.as_micros() as f64 / 1_000_000.0;
        self.flags.insert(ReceiverStatsFlags::HAS_NETWORK_LATENCY);
    }

    pub fn set_predict_offset(&mut self, diff_usec: i64) {
        self.predict_offset = diff_usec as f64 / 1_000_000.0;
        self.flags.insert(ReceiverStatsFlags::HAS_PREDICT_OFFSET);
    }
}
