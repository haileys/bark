use core::future::Future;

use bark_protocol::time::Timestamp;
use bark_protocol::types::AudioFrameF32;
use bark_protocol::packet::AudioData;

pub mod resample;
pub mod task;
pub use task::{Decode, NewDecodeError};

/// A timestamped audio segment. These are presented in order to the decoder
/// by the protocol layer.
pub struct AudioSegment {
    /// The presentation timestamp relative to the system clock of the decoder
    pub pts: Timestamp,
    /// Audio data
    pub data: AudioData,
}

#[derive(Debug, Clone, Copy)]
pub enum DecodeStatus {
    /// Decoder is synchronised to stream
    Sync,
    /// Decoder is slewing to resync with stream
    Slew,
    /// Decoder has no data
    Stall,
}

pub trait Receiver {
    /// Pull next [`AudioSegment`] to play. This is a demand for a segment -
    /// if there is no next segment available, `None` is returned and the
    /// segment is considered missed.
    fn next_segment(&self) -> Option<AudioSegment>;

    fn update_status(&self, status: DecodeStatus);
}

pub trait AudioSink {
    type WriteFuture<'a>: Future<Output = Timestamp> + 'a where Self: 'a;

    /// Writes audio data to the underlying sink. Returns the timestamp that
    /// the first frame of the passed data is expected to be played. This is
    /// a best-effort guess and is not required to be perfectly accurate,
    /// though it should be *on average* accurate over time. This is used to
    /// speed up or slow down playback to remain in sync.
    fn write<'a>(&'a mut self, audio: &'a [AudioFrameF32]) -> Self::WriteFuture<'a>;
}
