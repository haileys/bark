use core::time::Duration;

use bark_protocol::packet::Time;
use bark_protocol::time::ClockDelta;
use heapless::{HistoryBuffer, Vec};

const SAMPLE_HISTORY: usize = 64;

#[derive(Default)]
pub struct Timing {
    latency: Aggregate<Duration>,
    clock_delta: Aggregate<ClockDelta>,
}

#[allow(unused)]
impl Timing {
    pub fn receive_packet(&mut self, packet: Time) {
        let stream_1_usec = packet.data().stream_1.0;
        let stream_3_usec = packet.data().stream_3.0;

        let Some(rtt_usec) = stream_3_usec.checked_sub(stream_1_usec) else {
            // invalid packet, ignore
            return;
        };

        let network_latency = Duration::from_micros(rtt_usec / 2);
        self.latency.observe(network_latency);

        let clock_delta = ClockDelta::from_time_packet(&packet);
        self.clock_delta.observe(clock_delta);
    }

    pub fn network_latency(&self) -> Option<Duration> {
        self.latency.median()
    }

    pub fn clock_delta(&self) -> Option<ClockDelta> {
        self.clock_delta.median()
    }
}

#[derive(Default)]
pub struct Aggregate<T> {
    samples: HistoryBuffer<T, SAMPLE_HISTORY>
}

impl<T: Copy + Ord> Aggregate<T> {
    pub fn observe(&mut self, value: T) {
        self.samples.write(value);
    }

    pub fn median(&self) -> Option<T> {
        let mut samples = Vec::<T, SAMPLE_HISTORY>::new();
        samples.extend_from_slice(&self.samples).unwrap();
        samples.sort_unstable();
        samples.get(samples.len() / 2).copied()
    }
}
