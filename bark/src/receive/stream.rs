use bark_core::decode::AudioSegment;
use bark_core::receive::timing::Timing;
use bark_protocol::time::{TimestampDelta, Timestamp};
use bark_protocol::types::{SessionId, AudioPacketHeader};
use bark_protocol::packet::{Time, Audio};

use bark_core::receive::queue::PacketQueue;

pub struct Stream {
    sid: SessionId,
    timing: Timing,
    queue: PacketQueue,
}

impl Stream {
    pub fn new(header: &AudioPacketHeader) -> Self {
        // create packet queue data structure:
        let queue = PacketQueue::new(header);

        Stream {
            sid: header.sid,
            timing: Timing::default(),
            queue,
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
