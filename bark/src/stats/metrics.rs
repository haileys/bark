use std::sync::Arc;
use std::time::Duration;

use bark_protocol::time::{SampleDuration, TimestampDelta};

use super::value::{Counter, Gauge};

pub type ReceiverMetrics = Arc<ReceiverMetricsData>;
pub type SourceMetrics = Arc<SourceMetricsData>;

pub struct ReceiverMetricsData {
    pub audio_offset: Gauge<Option<TimestampDelta>>,
    pub buffer_delay: Gauge<SampleDuration>,
    pub buffer_underruns: Counter,
    pub queued_packets: Gauge<usize>,
    pub network_latency: Gauge<Duration>,
    pub packets_received: Counter,
    pub packets_lost: Counter,
    pub packets_missed: Counter,
    pub frames_decoded: Counter,
    pub frames_played: Counter,
}

impl ReceiverMetricsData {
    pub fn new() -> Self {
        Self {
            audio_offset: Gauge::new("bark_receiver_audio_offset_usec"),
            buffer_delay: Gauge::new("bark_receiver_buffer_delay_usec"),
            buffer_underruns: Counter::new("bark_receiver_buffer_underruns"),
            network_latency: Gauge::new("bark_receiver_network_latency_usec"),
            queued_packets: Gauge::new("bark_receiver_queued_packet_count"),
            packets_received: Counter::new("bark_receiver_packets_received"),
            packets_lost: Counter::new("bark_receiver_packets_lost"),
            packets_missed: Counter::new("bark_receiver_packets_missed"),
            frames_decoded: Counter::new("bark_receiver_frames_decoded"),
            frames_played: Counter::new("bark_receiver_frames_played"),
        }
    }
}

pub struct SourceMetricsData {}

impl SourceMetricsData {
    pub fn new() -> Self {
        Self {}
    }
}
