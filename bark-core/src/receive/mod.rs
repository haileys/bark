use core::future::Future;

use super::decode::AudioSegment;

pub mod consts;
pub mod queue;
pub mod timing;

pub trait OutputStream {
    /// Send audio segment to decoder.
    fn send_audio_segment(&self, segment: Option<AudioSegment>) -> Self::SendAudioSegmentFuture;
    type SendAudioSegmentFuture: Future<Output = ()> + Unpin;
}
