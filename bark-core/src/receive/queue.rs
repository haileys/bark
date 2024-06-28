use core::num::NonZeroU16;

use heapless::Deque;

use bark_protocol::packet::Audio;
use bark_protocol::types::AudioPacketHeader;
use bark_protocol::time::{SampleDuration, Timestamp};

use crate::consts::MAX_QUEUED_DECODE_SEGMENTS;

pub struct PacketQueue {
    queue: Deque<Option<AudioPts>, MAX_QUEUED_DECODE_SEGMENTS>,
    /// The seq of the first packet in the queue, the rest are implied
    head_seq: u64,
    /// We delay yielding packets when a queue is first started (or reset), to
    /// allow for some buffering. The amount of packets buffered depends on
    /// the difference between dts and pts in the initial packet.
    start: DelayStart,
}

#[derive(Debug)]
pub struct AudioPts {
    /// translated into local time:
    pub pts: Timestamp,
    pub audio: Audio,
}

impl AudioPts {
    pub fn header(&self) -> &AudioPacketHeader {
        self.audio.header()
    }
}

enum NoSlot {
    InPast,
    TooFarInFuture,
}

impl PacketQueue {
    pub fn new(initial: &AudioPacketHeader) -> Self {
        PacketQueue {
            queue: Deque::new(),
            head_seq: initial.seq,
            start: DelayStart::init(initial),
        }
    }

    pub fn pop_front(&mut self) -> Option<AudioPts> {
        if self.start.yield_packet() {
            self.head_seq += 1;
            self.queue.pop_front().flatten()
        } else {
            None
        }
    }

    pub fn insert_packet(&mut self, packet: AudioPts) {
        let packet_seq = packet.header().seq;
        let head_seq = self.head_seq;
        let tail_seq = self.head_seq + self.queue.capacity() as u64;

        match self.queue_slot_mut(packet_seq) {
            Ok(slot@&mut None) => {
                *slot = Some(packet);
            }
            Ok(Some(_)) => {
                log::warn!("received duplicate packet, retaining first received: packet_seq={packet_seq}");
            }
            Err(NoSlot::InPast) => {
                log::warn!("received packet in past, dropping: head_seq={head_seq}, packet_seq={packet_seq}");
            }
            Err(NoSlot::TooFarInFuture) => {
                log::warn!("received packet too far in future, resetting queue: tail_seq={tail_seq}, packet_seq={packet_seq}");

                // reset queue:
                self.head_seq = packet_seq;
                self.start = DelayStart::init(packet.header());
                self.queue.clear();
                self.queue.push_back(Some(packet)).expect("always room in queue after clear");

            }
        }
    }

    fn queue_slot_mut(&mut self, seq: u64) -> Result<&mut Option<AudioPts>, NoSlot> {
        let idx = seq.checked_sub(self.head_seq).ok_or(NoSlot::InPast)? as usize;

        if idx >= self.queue.capacity() {
            return Err(NoSlot::TooFarInFuture);
        }

        // expand deq if needed so we can take mut ref
        while self.queue.len() <= idx {
            let Ok(()) = self.queue.push_back(None) else {
                unreachable!("bounds check above implies this push always succeeds")
            };
        }

        let slices = self.queue.as_mut_slices();

        if idx < slices.0.len() {
            Ok(&mut slices.0[idx])
        } else {
            Ok(&mut slices.1[idx - slices.0.len()])
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

enum DelayStart {
    Delay(NonZeroU16),
    Live,
}

impl DelayStart {
    pub fn init(header: &AudioPacketHeader) -> Self {
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
        u16::try_from(packet_delay)
            .and_then(NonZeroU16::try_from)
            .map(DelayStart::Delay)
            .unwrap_or(DelayStart::Live)
    }

    pub fn yield_packet(&mut self) -> bool {
        if let DelayStart::Delay(count) = self {
            *self = NonZeroU16::new(count.get() - 1)
                .map(DelayStart::Delay)
                .unwrap_or(DelayStart::Live);
        }

        matches!(self, DelayStart::Live)
    }
}
