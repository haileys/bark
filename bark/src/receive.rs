use std::array;
use std::time::Duration;

use bytemuck::Zeroable;
use structopt::StructOpt;

use bark_core::receive::queue::AudioPts;

use bark_protocol::time::{Timestamp, SampleDuration, TimestampDelta, ClockDelta};
use bark_protocol::types::{AudioPacketHeader, ReceiverId, SessionId, TimePhase, TimestampMicros};
use bark_protocol::types::stats::receiver::ReceiverStats;
use bark_protocol::packet::{Audio, Time, PacketKind, StatsReply};

use crate::audio::config::{DEFAULT_PERIOD, DEFAULT_BUFFER, DeviceOpt};
use crate::audio::Output;
use crate::receive::output::OutputRef;
use crate::socket::{ProtocolSocket, Socket, SocketOpt};
use crate::{time, stats, thread};
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
    clock_delta: Aggregate<ClockDelta>,
    predict_offset: Aggregate<i64>,
}

impl Stream {
    pub fn new(header: &AudioPacketHeader, output: OutputRef) -> Self {
        let decode = DecodeStream::new(header, output);

        Stream {
            sid: header.sid,
            decode,
            latency: Aggregate::new(),
            clock_delta: Aggregate::new(),
            predict_offset: Aggregate::new(),
        }
    }

    pub fn adjust_pts(&self, pts: Timestamp) -> Option<Timestamp> {
        self.clock_delta.median().map(|delta| {
            pts.adjust(TimestampDelta::from_clock_delta_lossy(delta))
        })
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

    pub fn receive_time(&mut self, packet: Time) {
        let Some(stream) = self.stream.as_mut() else {
            // no stream, nothing we can do with a time packet
            return;
        };

        if stream.sid != packet.data().sid {
            // not relevant to our stream, ignore
            return;
        }

        let stream_1_usec = packet.data().stream_1.0;
        let stream_3_usec = packet.data().stream_3.0;

        let Some(rtt_usec) = stream_3_usec.checked_sub(stream_1_usec) else {
            // invalid packet, ignore
            return;
        };

        let network_latency = Duration::from_micros(rtt_usec / 2);
        stream.latency.observe(network_latency);

        let clock_delta = ClockDelta::from_time_packet(&packet);
        stream.clock_delta.observe(clock_delta);
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
        let stream = self.prepare_stream(header);

        let packet_dts = header.dts;

        // translate presentation timestamp of this packet:
        let pts = Timestamp::from_micros_lossy(header.pts);
        let pts = stream.adjust_pts(pts).unwrap_or_else(|| {
            // if we don't yet have the clock information to adjust timestamps,
            // default to packet pts-dts, added to our current local time
            let stream_delay = header.pts.0.saturating_sub(header.dts.0);
            Timestamp::from_micros_lossy(TimestampMicros(now.0 + stream_delay))
        });

        // TODO - this is where we would take buffer length stats
        stream.decode.send(AudioPts {
            pts,
            audio: packet,
        })?;

        if let Some(latency) = stream.network_latency() {
            if let Some(clock_delta) = stream.clock_delta.median() {
                let latency_usec = u64::try_from(latency.as_micros()).unwrap();
                let delta_usec = clock_delta.as_micros();
                let predict_dts = (now.0 - latency_usec).checked_add_signed(-delta_usec).unwrap();
                let predict_diff = predict_dts as i64 - packet_dts.0 as i64;
                stream.predict_offset.observe(predict_diff);
            }
        }

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
    let receiver_id = generate_receiver_id();
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
            Some(PacketKind::Time(mut time)) => {
                if !time.data().rid.matches(&receiver_id) {
                    // not for us - time packets are usually unicast,
                    // but there can be multiple receivers on a machine
                    continue;
                }

                match time.data().phase() {
                    Some(TimePhase::Broadcast) => {
                        let data = time.data_mut();
                        data.receive_2 = time::now();
                        data.rid = receiver_id;

                        protocol.send_to(time.as_packet(), peer)
                            .expect("reply to time packet");
                    }
                    Some(TimePhase::StreamReply) => {
                        // let mut state = state.lock().unwrap();
                        receiver.receive_time(time);
                    }
                    _ => {
                        // not for us - must be destined for another process
                        // on same machine
                    }
                }
            }
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

pub fn generate_receiver_id() -> ReceiverId {
    ReceiverId(rand::random())
}
