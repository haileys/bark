use std::array;
use std::time::Duration;

use bytemuck::Zeroable;
use structopt::StructOpt;

use bark_core::receive::queue::AudioPts;

use bark_protocol::time::{Timestamp, SampleDuration};
use bark_protocol::types::{AudioPacketHeader, SessionId};
use bark_protocol::types::stats::receiver::ReceiverStats;
use bark_protocol::packet::{Audio, PacketKind, StatsReply};

use crate::audio::config::{DEFAULT_PERIOD, DEFAULT_BUFFER, DeviceOpt};
use crate::audio::Output;
use crate::receive::output::OutputRef;
use crate::socket::{ProtocolSocket, Socket, SocketOpt};
use crate::{stats, thread, time};
use crate::RunError;

use self::output::OwnedOutput;
use self::queue::Disconnected;
use self::stream::DecodeStream;

pub mod output;
pub mod queue;
pub mod stream;

pub struct Receiver {
    stream: Option<Stream>,
    output: OwnedOutput,
}

struct Stream {
    sid: SessionId,
    decode: DecodeStream,
    latency: Aggregate<Duration>,
    predict_offset: Aggregate<i64>,
}

impl Stream {
    pub fn new(header: &AudioPacketHeader, output: OutputRef) -> Self {
        let decode = DecodeStream::new(header, output);

        Stream {
            sid: header.sid,
            decode,
            latency: Aggregate::new(),
            predict_offset: Aggregate::new(),
        }
    }

    pub fn network_latency(&self) -> Option<Duration> {
        self.latency.median()
    }

    pub fn predict_offset(&self) -> Option<i64> {
        self.predict_offset.median()
    }
}

impl Receiver {
    pub fn new(output: Output) -> Self {
        Receiver {
            stream: None,
            output: OwnedOutput::new(output),
        }
    }

    pub fn stats(&self) -> ReceiverStats {
        let mut stats = ReceiverStats::new();

        if let Some(stream) = &self.stream {
            let decode = stream.decode.stats();
            stats.set_stream(decode.status);
            stats.set_buffer_length(decode.buffered);
            stats.set_audio_latency(decode.audio_latency);
            stats.set_output_latency(decode.output_latency);

            if let Some(latency) = stream.network_latency() {
                stats.set_network_latency(latency);
            }

            if let Some(predict) = stream.predict_offset() {
                stats.set_predict_offset(predict);
            }
        }

        stats
    }

    pub fn current_session(&self) -> Option<SessionId> {
        self.stream.as_ref().map(|s| s.sid)
    }

    fn prepare_stream(&mut self, header: &AudioPacketHeader) -> &mut Stream {
        let new_stream = match &self.stream {
            Some(stream) => stream.sid < header.sid,
            None => true,
        };

        if new_stream {
            // start new stream
            let stream = Stream::new(header, self.output.steal());

            // new stream is taking over! switch over to it
            log::info!("new stream beginning: sid={}", header.sid.0);
            self.stream = Some(stream);
        }

        self.stream.as_mut().unwrap()
    }

    pub fn receive_audio(&mut self, packet: Audio) -> Result<(), Disconnected> {
        let now = time::now();

        let header = packet.header();
        let packet_dts = header.dts;

        let stream = self.prepare_stream(header);

        // translate presentation timestamp of this packet:
        let pts = Timestamp::from_micros_lossy(header.pts);

        // TODO - this is where we would take buffer length stats
        stream.decode.send(AudioPts {
            pts,
            audio: packet,
        })?;

        let latency_usec = now.0.saturating_sub(packet_dts.0);
        stream.latency.observe(Duration::from_micros(latency_usec));

        Ok(())
    }
}

struct Aggregate<T> {
    samples: [T; 64],
    count: usize,
    index: usize,
}

impl<T: Copy + Default + Ord> Aggregate<T> {
    pub fn new() -> Self {
        let samples = array::from_fn(|_| Default::default());
        Aggregate { samples, count: 0, index: 0 }
    }

    pub fn observe(&mut self, value: T) {
        self.samples[self.index] = value;

        if self.count < self.samples.len() {
            self.count += 1;
        }

        self.index += 1;
        self.index %= self.samples.len();
    }

    pub fn median(&self) -> Option<T> {
        let mut samples = self.samples;
        let samples = &mut samples[0..self.count];
        samples.sort();
        samples.get(self.count / 2).copied()
    }
}

#[derive(StructOpt, Clone)]
pub struct ReceiveOpt {
    #[structopt(flatten)]
    pub socket: SocketOpt,

    /// Audio device name
    #[structopt(long, env = "BARK_RECEIVE_OUTPUT_DEVICE")]
    pub output_device: Option<String>,

    /// Size of discrete audio transfer buffer in frames
    #[structopt(long, env = "BARK_RECEIVE_OUTPUT_PERIOD")]
    pub output_period: Option<u64>,

    /// Size of decoded audio buffer in frames
    #[structopt(long, env = "BARK_RECEIVE_OUTPUT_BUFFER")]
    pub output_buffer: Option<u64>,
}

pub fn run(opt: ReceiveOpt) -> Result<(), RunError> {
    let node = stats::node::get();

    let output = Output::new(&DeviceOpt {
        device: opt.output_device,
        period: opt.output_period
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_PERIOD),
        buffer: opt.output_buffer
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_BUFFER),
    }).map_err(RunError::OpenAudioDevice)?;

    let mut receiver = Receiver::new(output);

    let socket = Socket::open(opt.socket)
        .map_err(RunError::Listen)?;

    let protocol = ProtocolSocket::new(socket);

    thread::set_name("bark/network");
    thread::set_realtime_priority();

    loop {
        let (packet, peer) = protocol.recv_from().map_err(RunError::Receive)?;

        match packet.parse() {
            Some(PacketKind::Audio(packet)) => {
                receiver.receive_audio(packet)?;
            }
            Some(PacketKind::StatsRequest(_)) => {
                // let state = state.lock().unwrap();
                let sid = receiver.current_session().unwrap_or(SessionId::zeroed());
                let receiver = receiver.stats();
                println!("{:?}", receiver);

                let reply = StatsReply::receiver(sid, receiver, node)
                    .expect("allocate StatsReply packet");

                let _ = protocol.send_to(reply.as_packet(), peer);
            }
            Some(PacketKind::StatsReply(_)) => {
                // ignore
            }
            None => {
                // unknown packet type, ignore
            }
        }
    }
}
