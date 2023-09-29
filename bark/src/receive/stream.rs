use std::num::NonZeroU16;

use bark_core::decode::AudioSegment;
use bark_core::receive::timing::Timing;
use bark_protocol::time::{TimestampDelta, Timestamp, SampleDuration};
use bark_protocol::types::{SessionId, AudioPacketHeader};
use bark_protocol::packet::{Time, Audio};

use bark_core::receive::queue::PacketQueue;

pub struct Stream {
    sid: SessionId,
    timing: Timing,
    queue: PacketQueue,
    start: DelayStart,
}

enum DelayStart {
    Delay(NonZeroU16),
    Live,
}

impl Stream {
    pub fn new(header: &AudioPacketHeader) -> Self {
        // calculate the stream delay by taking the difference between
        // pts and dts in the initial packet:
        let initial_pts = Timestamp::from_micros_lossy(header.pts);
        let initial_dts = Timestamp::from_micros_lossy(header.dts);
        let delay = initial_pts.saturating_duration_since(initial_dts);

        // calculate number of packets this delay represents:
        let packet_delay = delay.to_frame_count() / SampleDuration::ONE_PACKET.to_frame_count();
        // quick n dirty round up:
        let packet_delay = packet_delay + 1;
        // calculate how many packets we should wait for before starting to
        // yield audio segments to the decoder. this allows some time to build
        // a buffer before beginning:
        let start = u16::try_from(packet_delay)
            .and_then(NonZeroU16::try_from)
            .map(DelayStart::Delay)
            .unwrap_or(DelayStart::Live);

        // create packet queue data structure:
        let queue = PacketQueue::new(header.seq);

        Stream {
            sid: header.sid,
            timing: Timing::default(),
            queue,
            start,
        }
    }

    pub fn sid(&self) -> SessionId {
        self.sid
    }

    pub fn receive_time(&mut self, packet: Time) {
        self.timing.receive_packet(packet);
    }

    pub fn receive_audio(&mut self, packet: Audio) {
        self.queue.insert_packet(packet);
    }

    pub fn next_audio_segment(&mut self) -> Option<AudioSegment> {
        // check delay start,
        // decrement 1 from remaining packet count if in delay
        match self.start {
            DelayStart::Live => {}
            DelayStart::Delay(count) => {
                self.start = NonZeroU16::new(count.get() - 1)
                    .map(DelayStart::Delay)
                    .unwrap_or(DelayStart::Live);

                return None;
            }
        }

        let packet = self.queue.pop_front()?;

        // if we haven't received any timing information yet, play it
        // safe and emit None for this segment, better than playing out
        // of sync audio
        let delta = self.timing.clock_delta()?;
        let delta = TimestampDelta::from_clock_delta_lossy(delta);

        let pts = Timestamp::from_micros_lossy(packet.header().pts).adjust(delta);

        Some(AudioSegment { pts, data: packet.into_data() })
    }
}
