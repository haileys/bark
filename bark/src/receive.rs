use std::time::Duration;

use bark_core::audio::{Format, F32, S16};
use bytemuck::Zeroable;
use structopt::StructOpt;

use bark_core::receive::queue::AudioPts;

use bark_protocol::time::{Timestamp, SampleDuration};
use bark_protocol::types::{AudioPacketHeader, SessionId, TimestampMicros};
use bark_protocol::types::stats::receiver::ReceiverStats;
use bark_protocol::packet::{Audio, PacketKind, Pong, StatsReply};

use crate::audio::config::{DEFAULT_PERIOD, DEFAULT_BUFFER, DeviceOpt};
use crate::audio::Output;
use crate::config;
use crate::receive::output::OutputRef;
use crate::socket::{ProtocolSocket, Socket, SocketOpt};
use crate::stats::{self, ReceiverMetrics};
use crate::{thread, time};
use crate::RunError;

use self::output::OwnedOutput;
use self::queue::Disconnected;
use self::stream::DecodeStream;

pub mod output;
pub mod queue;
pub mod stream;

pub struct Receiver<F: Format> {
    stream: Option<Stream>,
    output: OwnedOutput<F>,
    metrics: ReceiverMetrics,
}

struct Stream {
    sid: SessionId,
    decode: DecodeStream,
    receieved_last_packet: TimestampMicros,
    priority: i8,
}

const STREAM_TIMEOUT: Duration = Duration::from_millis(100);

impl Stream {
    pub fn new<F: Format>(
        header: &AudioPacketHeader,
        output: OutputRef<F>,
        metrics: ReceiverMetrics,
        now: TimestampMicros,
    ) -> Self {
        let decode = DecodeStream::new(header, output, metrics);

        Stream {
            sid: header.sid,
            decode,
            receieved_last_packet: now,
            priority: header.priority,
        }
    }

    pub fn is_active(&self, now: TimestampMicros) -> bool {
        self.receieved_last_packet > now.saturating_sub(STREAM_TIMEOUT)
    }

    pub fn receive_packet(&mut self, audio: Audio, now: TimestampMicros) -> Result<(), Disconnected> {
        let pts = Timestamp::from_micros_lossy(audio.header().pts);
        self.decode.send(AudioPts { pts, audio })?;
        self.receieved_last_packet = now;
        Ok(())
    }
}

impl<F: Format> Receiver<F> {
    pub fn new(output: Output<F>, metrics: ReceiverMetrics) -> Self {
        Receiver {
            stream: None,
            output: OwnedOutput::new(output),
            metrics,
        }
    }

    pub fn stats(&self) -> ReceiverStats {
        let mut stats = ReceiverStats::new();

        if let Some(stream) = &self.stream {
            let decode = stream.decode.stats();
            stats.set_stream(decode.status);
            stats.set_audio_latency(decode.audio_latency);
            stats.set_output_latency(decode.output_latency);

            let latency = self.metrics.network_latency.get()
                .and_then(|micros| u64::try_from(micros).ok())
                .map(Duration::from_micros);

            if let Some(latency) = latency {
                stats.set_network_latency(latency);
            }
        }

        stats
    }

    pub fn current_session(&self) -> Option<SessionId> {
        self.stream.as_ref().map(|s| s.sid)
    }

    fn prepare_stream(&mut self, header: &AudioPacketHeader, now: TimestampMicros) -> &mut Stream {
        let new_stream = match &self.stream {
            Some(current) if current.is_active(now) => {
                if header.priority > current.priority {
                    true
                } else if header.priority == current.priority {
                    header.sid > current.sid
                } else {
                    false
                }
            }
            _ => true,
        };

        if new_stream {
            // start new stream
            let stream = Stream::new(header, self.output.steal(), self.metrics.clone(), now);

            // new stream is taking over! switch over to it
            log::info!("new stream beginning: priority={} sid={}", header.priority, header.sid.0);
            self.stream = Some(stream);
        }

        self.stream.as_mut().unwrap()
    }

    pub fn receive_audio(&mut self, packet: Audio) -> Result<(), Disconnected> {
        let now = time::now();

        let header = packet.header();
        let dts = header.dts;

        // prepare stream for incoming packet
        let stream = self.prepare_stream(header, now);

        // if packet does not match current stream, exit early
        if header.sid != stream.sid {
            return Ok(());
        }

        // feed packet to stream
        stream.receive_packet(packet, now)?;

        // update metrics
        let latency = now.saturating_duration_since(dts);
        self.metrics.network_latency.observe(latency);
        self.metrics.packets_received.increment();

        Ok(())
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
    pub output_period: Option<usize>,

    /// Size of decoded audio buffer in frames
    #[structopt(long, env = "BARK_RECEIVE_OUTPUT_BUFFER")]
    pub output_buffer: Option<usize>,

    #[structopt(long, env = "BARK_RECEIVE_OUTPUT_FORMAT", default_value = "f32")]
    pub output_format: config::Format,
}

pub async fn run(opt: ReceiveOpt, metrics: stats::server::MetricsOpt) -> Result<(), RunError> {
    let socket = Socket::open(&opt.socket)
        .map_err(RunError::Listen)?;

    let metrics = stats::server::start_receiver(&metrics).await?;

    match opt.output_format {
        config::Format::S16 => run_format::<S16>(opt, socket, metrics).await,
        config::Format::F32 => run_format::<F32>(opt, socket, metrics).await,
    }
}

async fn run_format<F: Format>(
    opt: ReceiveOpt,
    socket: Socket,
    metrics: stats::ReceiverMetrics,
) -> Result<(), RunError> {
    let device_opt = DeviceOpt {
        device: opt.output_device,
        period: opt.output_period
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_PERIOD),
        buffer: opt.output_buffer
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_BUFFER),
    };

    let output = Output::<F>::new(&device_opt, metrics.clone())
        .map_err(RunError::OpenAudioDevice)?;

    let receiver = Receiver::new(output, metrics.clone());

    thread::start("bark/network", move || {
        network_thread(socket, receiver)
    }).await
}

fn network_thread<F: Format>(
    socket: Socket,
    mut receiver: Receiver<F>,
) -> Result<(), RunError> {
    thread::set_realtime_priority();

    let node = stats::node::get();
    let protocol = ProtocolSocket::new(socket);

    loop {
        let (packet, peer) = protocol.recv_from().map_err(RunError::Receive)?;

        match packet.parse() {
            Some(PacketKind::Audio(packet)) => {
                receiver.receive_audio(packet)?;
            }
            Some(PacketKind::StatsRequest(_)) => {
                let sid = receiver.current_session().unwrap_or(SessionId::zeroed());
                let receiver = receiver.stats();

                let reply = StatsReply::receiver(sid, receiver, node)
                    .expect("allocate StatsReply packet");

                let _ = protocol.send_to(reply.as_packet(), peer);
            }
            Some(PacketKind::StatsReply(_)) => {
                // ignore
            }
            Some(PacketKind::Ping(_)) => {
                let pong = Pong::new().expect("allocate Pong packet");
                let _ = protocol.send_to(pong.as_packet(), peer);
            }
            Some(PacketKind::Pong(_)) => {
                // ignore
            }
            None => {
                // unknown packet type, ignore
            }
        }
    }
}
