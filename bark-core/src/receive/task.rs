use core::future::Pending;
use core::pin::Pin;
use core::task::{Context, Poll};

use bark_protocol::time::{Timestamp, TimestampDelta};
use futures::ready;
use futures::future::{self, FutureExt};

use bark_protocol::packet::{Packet, PacketKind, Audio, Time};
use bark_protocol::types::{SessionId, TimePhase};

use crate::decode::AudioSegment;

use super::timing::Timing;
use super::{Platform, OutputStream};
use super::queue::PacketQueue;

pub async fn run<P: Platform>(platform: P) -> ! {
    let mut receive = Receive::new(&platform);

    future::poll_fn(move |cx| {
        receive.poll(cx);
        Poll::Pending
    }).await
}

struct Receive<'a, P: Platform> {
    platform: &'a P,
    network: NetworkTask<'a, P>,
    stream: Option<ReceiveStream<P::OutputStream>>,
}

impl<'a, P: Platform> Receive<'a, P> {
    pub fn new(platform: &'a P) -> Self {
        Receive {
            platform,
            network: NetworkTask::new(platform),
            stream: None,
        }
    }

    pub fn poll(&mut self, cx: &mut Context) {
        self.poll_network(cx);

        if let Some(stream) = self.stream.as_mut() {
            stream.poll(cx);
        }
    }

    fn poll_network(&mut self, cx: &mut Context) {
        match self.network.poll(cx) {
            Poll::Ready(NetworkEvent::Audio(packet)) => {
                let header = packet.header();

                // compare packet sid with current stream sid to see if there
                // is a new stream, reset if so:
                if let Some(stream) = self.stream.as_mut() {
                    if stream.sid < header.sid {
                        self.stream = None;
                    }
                }

                // send packet to current stream if there is one, create a new
                // stream if not
                if let Some(stream) = self.stream.as_mut() {
                    stream.receive_audio(packet);
                } else {
                    let output = self.platform.start_output_stream();
                    self.stream = Some(ReceiveStream::new(packet, output));
                }
            }
            Poll::Ready(NetworkEvent::Time(packet)) => {
                // see if time packet matches current stream:
                let stream = self.stream.as_mut()
                    .filter(|stream| stream.sid == packet.data().sid);

                // send time packet to stream if so:
                if let Some(stream) = stream {
                    stream.receive_time(packet);
                }
            }
            Poll::Pending => {}
        }
    }
}

enum NetworkEvent {
    Audio(Audio),
    Time(Time),
}

struct NetworkTask<'a, P> {
    platform: &'a P,
}

impl<'a, P: Platform> NetworkTask<'a, P> {
    pub fn new(platform: &'a P) -> Self {
        NetworkTask { platform }
    }

    pub fn poll(&mut self, cx: &Context) -> Poll<NetworkEvent> {
        let (packet, peer) = ready!(self.poll_packet(cx));

        match packet {
            PacketKind::Audio(audio) => {
                let event = NetworkEvent::Audio(audio);
                return Poll::Ready(event);
            }
            PacketKind::Time(mut time) => {
                match time.data().phase() {
                    Some(TimePhase::Broadcast) => {
                        let data = time.data_mut();
                        data.receive_2 = self.platform.current_time();
                        let buffer = time.into_packet().into_buffer();
                        self.platform.send_packet(buffer, peer);
                    }
                    Some(TimePhase::StreamReply) => {
                        let event = NetworkEvent::Time(time);
                        return Poll::Ready(event);
                    }
                    _ => {
                        // invalid time packet
                    }
                }
            }
            _ => {
                log::warn!("received unhandled packet kind: {packet:?}");
            }
        }

        Poll::Pending
    }

    fn poll_packet(&mut self, cx: &Context) -> Poll<(PacketKind, P::PeerId)> {
        let (buffer, peer) = ready!(self.platform.poll_receive_packet(cx));

        let packet = Packet::from_buffer(buffer)
            .and_then(|packet| packet.parse());

        match packet {
            Some(packet) => Poll::Ready((packet, peer)),
            None => Poll::Pending,
        }
    }
}

struct ReceiveStream<O: OutputStream> {
    sid: SessionId,
    timing: Timing,
    queue: PacketQueue,
    output: O,
    output_fut: Option<O::SendAudioSegmentFuture>,
}

impl<O: OutputStream> ReceiveStream<O> {
    pub fn new(packet: Audio, output: O) -> Self {
        let header = packet.header();

        ReceiveStream {
            sid: header.sid,
            timing: Timing::default(),
            queue: PacketQueue::new(header.seq),
            output,
            output_fut: None,
        }
    }

    pub fn receive_audio(&mut self, audio: Audio) {
        self.queue.insert_packet(audio);
    }

    pub fn receive_time(&mut self, time: Time) {
        self.timing.receive_packet(time);
    }

    pub fn poll(&mut self, cx: &mut Context) {
        if let Some(fut) = self.output_fut.as_mut() {
            if fut.poll_unpin(cx).is_pending() {
                return;
            }
        }

        let segment = self.next_segment();
        self.output_fut = Some(self.output.send_audio_segment(segment));
    }

    fn next_segment(&mut self) -> Option<AudioSegment> {
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
