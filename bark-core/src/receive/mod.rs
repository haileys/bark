use core::future::Future;
use core::task::{Context, Poll};

use bark_protocol::buffer::PacketBuffer;
use bark_protocol::types::TimestampMicros;

use super::decode::AudioSegment;

pub mod consts;
pub mod queue;
// pub mod task;
pub mod timing;

pub trait Platform {
    /// Peer address for sending and receiving packets, for example
    /// [`core::net::SocketAddrV4`].
    type PeerId;

    /// Receive packet from network.
    fn poll_receive_packet(&self, cx: &Context) -> Poll<(PacketBuffer, Self::PeerId)>;

    /// Send packet to peer. Packet should be sent immediately, not queued.
    /// This function never blocks.
    fn send_packet(&self, packet: PacketBuffer, addr: Self::PeerId);

    /// Get current system time. This should be a monotonic clock not subject
    /// to NTP adjustments, like `CLOCK_BOOTTIME` on Linux.
    fn current_time(&self) -> TimestampMicros;

    /// Start a new audio output stream.
    fn start_output_stream(&self) -> Self::OutputStream;
    type OutputStream: OutputStream;
}

pub trait OutputStream {
    /// Send audio segment to decoder.
    fn send_audio_segment(&self, segment: Option<AudioSegment>) -> Self::SendAudioSegmentFuture;
    type SendAudioSegmentFuture: Future<Output = ()> + Unpin;
}
