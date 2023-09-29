use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bark_core::decode::{DecodeStatus, Decode};
use bark_network::{Socket, ProtocolSocket};
use bark_protocol::packet::PacketKind;
use bark_protocol::time::SampleDuration;
use bark_protocol::types::{ReceiverId, SessionId, TimePhase};
use structopt::StructOpt;

use crate::stats;
use crate::{SocketOpt, RunError};

use self::stream::Stream;

mod consts;
// mod queue;
mod stream;

#[derive(StructOpt, Clone)]
pub struct ReceiveOpt {
    #[structopt(flatten)]
    pub socket: SocketOpt,
    #[structopt(long, env = "BARK_RECEIVE_DEVICE")]
    pub device: Option<String>,
    #[structopt(long, default_value="10")]
    pub buffer_latency_ms: u64,
}

pub fn run(opt: ReceiveOpt) -> Result<(), RunError> {
    let _node = stats::node::get();

    if let Some(device) = &opt.device {
        bark_device::env::set_sink(device);
    }

    let socket = Socket::open(opt.socket.multicast)
        .map_err(RunError::Listen)?;

    let protocol = ProtocolSocket::new(socket);
    let receiver = ReceiverRef::new();

    let buffer_latency = Duration::from_millis(opt.buffer_latency_ms);
    let buffer_latency = SampleDuration::from_std_duration_lossy(buffer_latency);
    let sink = bark_device::sink::open(buffer_latency)
        .map_err(RunError::OpenDevice)?;

    let decode = Decode::new(receiver.clone(), sink)?;
    start_decode_thread(decode);

    loop {
        let (packet, addr) = match protocol.recv_from() {
            Ok(result) => result,
            Err(e) => {
                log::warn!("receiving network packet: {e:?}");
                continue;
            }
        };

        match packet {
            PacketKind::Audio(audio) => {
                let header = audio.header();
                let mut receiver = receiver.lock();
                let stream = receiver.prepare_stream(header.sid, header.seq);
                stream.receive_audio(audio);
            }
            PacketKind::Time(mut time) => {
                match time.data().phase() {
                    Some(TimePhase::Broadcast) => {
                        let data = time.data_mut();
                        data.receive_2 = bark_util::time::now();
                        match protocol.send_to(time.as_packet(), addr) {
                            Ok(()) => {}
                            Err(e) => {
                                log::warn!("replying to time broadcast: {e:?}");
                            }
                        }
                    }
                    Some(TimePhase::StreamReply) => {
                        let data = time.data();
                        let mut receiver = receiver.lock();
                        if let Some(stream) = receiver.get_stream(data.sid) {
                            stream.receive_time(time);
                        }
                    }
                    _ => { /* invalid packet */ }
                }
            }
            _ => {
                log::warn!("received unhandled packet kind: {packet:?}");
            }
        }
    }
}

fn start_decode_thread<R, S>(decode: Decode<R, S>) where
    R: bark_core::decode::Receiver + Send + 'static,
    S: bark_core::decode::AudioSink + Send + 'static,
{
    bark_util::thread::start("bark/decode", || {
        futures::executor::block_on(async move {
            decode.run().await;
        })
    })
}

#[derive(Clone)]
pub struct ReceiverRef(Arc<Mutex<Receiver>>);

impl ReceiverRef {
    pub fn new() -> Self {
        ReceiverRef(Arc::new(Mutex::new(Receiver::new())))
    }

    pub fn lock(&self) -> MutexGuard<'_, Receiver> {
        self.0.lock().unwrap()
    }
}

pub struct Receiver {
    #[allow(unused)]
    id: ReceiverId,
    stream: Option<Stream>,
}

impl Receiver {
    pub fn new() -> Self {
        Receiver {
            id: ReceiverId(rand::random()),
            stream: None,
        }
    }

    fn get_stream(&mut self, sid: SessionId) -> Option<&mut Stream> {
        self.stream.as_mut().filter(|stream| stream.sid() == sid)
    }

    /// Resets current stream if necessary.
    fn prepare_stream(&mut self, sid: SessionId, seq: u64) -> &mut Stream {
        let new_stream = match &self.stream {
            Some(stream) => stream.sid() < sid,
            None => true,
        };

        if new_stream {
            self.stream = Some(Stream::new(sid, seq));
        }

        self.stream.as_mut().unwrap()
    }
}

impl bark_core::decode::Receiver for ReceiverRef {
    fn next_segment(&self) -> Option<bark_core::decode::AudioSegment> {
        let mut receiver = self.lock();
        let stream = receiver.stream.as_mut()?;
        stream.next_audio_segment()
    }

    fn update_status(&self, _status: DecodeStatus) {
        // TODO
    }
}
