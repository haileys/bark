use std::array;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bark_core::audio::Frame;
use bark_core::receive::pipeline::Pipeline;
use bark_core::receive::timing::Timing;
use bytemuck::Zeroable;
use structopt::StructOpt;

use bark_core::receive::queue::PacketQueue;

use bark_protocol::FRAMES_PER_PACKET;
use bark_protocol::time::{Timestamp, SampleDuration, TimestampDelta, ClockDelta};
use bark_protocol::types::{SessionId, ReceiverId, TimePhase, AudioPacketHeader};
use bark_protocol::types::stats::receiver::{ReceiverStats, StreamStatus};
use bark_protocol::packet::{Audio, Time, PacketKind, StatsReply};

use crate::audio::config::{DEFAULT_PERIOD, DEFAULT_BUFFER, DeviceOpt};
use crate::audio::output::Output;
use crate::socket::{ProtocolSocket, Socket, SocketOpt};
use crate::{time, stats, thread};
use crate::RunError;

pub struct Receiver {
    stats: ReceiverStats,
    stream: Option<Stream>,
}

struct Stream {
    sid: SessionId,
    latency: Aggregate<Duration>,
    clock_delta: Aggregate<ClockDelta>,
    queue: PacketQueue,
    pipeline: Pipeline,
}

impl Stream {
    pub fn new(header: &AudioPacketHeader) -> Self {
        let queue = PacketQueue::new(header);


        Stream {
            sid: header.sid,
            latency: Aggregate::new(),
            clock_delta: Aggregate::new(),
            queue,
            pipeline: Pipeline::new(header),
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
}

#[derive(Clone, Copy)]
pub struct ClockInfo {
    pub network_latency_usec: i64,
    pub clock_diff_usec: i64,
}

impl Receiver {
    pub fn new() -> Self {
        Receiver {
            stream: None,
            stats: ReceiverStats::new(),
        }
    }

    pub fn stats(&self) -> &ReceiverStats {
        &self.stats
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

        if let Some(latency) = stream.network_latency() {
            self.stats.set_network_latency(latency);
        }

        let clock_delta = ClockDelta::from_time_packet(&packet);
        stream.clock_delta.observe(clock_delta);
    }

    fn prepare_stream(&mut self, header: &AudioPacketHeader) -> &mut Stream {
        let new_stream = match &self.stream {
            Some(stream) => stream.sid < header.sid,
            None => true,
        };

        if new_stream {
            // new stream is taking over! switch over to it
            log::info!("new stream beginning: sid={}", header.sid.0);
            self.stream = Some(Stream::new(header));
            self.stats.clear();
        }

        self.stream.as_mut().unwrap()
    }

    pub fn receive_audio(&mut self, packet: Audio) {
        let now = time::now();

        let packet_dts = packet.header().dts;

        let stream = self.prepare_stream(packet.header());
        stream.queue.insert_packet(packet);

        if let Some(latency) = stream.network_latency() {
            if let Some(clock_delta) = stream.clock_delta.median() {
                let latency_usec = u64::try_from(latency.as_micros()).unwrap();
                let delta_usec = clock_delta.as_micros();
                let predict_dts = (now.0 - latency_usec).checked_add_signed(-delta_usec).unwrap();
                let predict_diff = predict_dts as i64 - packet_dts.0 as i64;
                self.stats.set_predict_offset(predict_diff)
            }
        }
    }

    pub fn write_audio(&mut self, buffer: &mut [Frame], pts: Timestamp) -> usize {
        // get stream start timing information:
        let Some(stream) = self.stream.as_mut() else {
            // stream hasn't started, just fill buffer with silence and return
            buffer[0..FRAMES_PER_PACKET].fill(Frame::zeroed());
            return FRAMES_PER_PACKET;
        };

        // get next packet from queue, or None if missing (packet loss)
        let packet = stream.queue.pop_front();

        // calculate stream timing from packet timing info if present
        let header_pts = packet.as_ref()
            .map(|packet| packet.header().pts)
            .map(Timestamp::from_micros_lossy);

        let stream_pts = header_pts
            .and_then(|header_pts| stream.adjust_pts(header_pts));

        let timing = stream_pts.map(|stream_pts| Timing {
            real: pts,
            play: stream_pts,
        });

        // adjust resampler rate based on stream timing info
        if let Some(timing) = timing {
            stream.pipeline.set_timing(timing);

            if stream.pipeline.slew() {
                self.stats.set_stream(StreamStatus::Slew);
            } else {
                self.stats.set_stream(StreamStatus::Sync);
            }

            self.stats.set_audio_latency(timing.real, timing.play);
        }

        // pass packet through decode pipeline
        let frames = stream.pipeline.process(packet.as_ref(), buffer);

        // report stats and return
        self.stats.set_buffer_length(
            SampleDuration::from_frame_count(
                (FRAMES_PER_PACKET * stream.queue.len()).try_into().unwrap()));

        frames
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

    struct SharedState {
        pub recv: Receiver,
    }

    let output = Output::new(DeviceOpt {
        device: opt.output_device,
        period: opt.output_period
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_PERIOD),
        buffer: opt.output_buffer
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_BUFFER),
    }).map_err(RunError::OpenAudioDevice)?;

    let state = Arc::new(Mutex::new(SharedState {
        recv: Receiver::new(),
    }));

    std::thread::spawn({
        let state = state.clone();
        move || {
            thread::set_name("bark/audio");
            thread::set_realtime_priority();

            loop {
                let mut state = state.lock().unwrap();

                let delay = output.delay().unwrap();
                state.recv.stats.set_output_latency(delay);

                let pts = time::now();
                let pts = Timestamp::from_micros_lossy(pts);
                let pts = pts.add(delay);

                // this should be large enough for `write_audio` to process an
                // entire packet with:
                let mut buffer = [Frame::zeroed(); FRAMES_PER_PACKET * 2];
                let count = state.recv.write_audio(&mut buffer, pts);

                // drop lock before calling `Output::write` (blocking!)
                drop(state);

                // send audio to ALSA
                match output.write(&buffer[0..count]) {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("error playing audio: {e}");
                        break;
                    }
                };
            }
        }
    });

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
                        let mut state = state.lock().unwrap();
                        state.recv.receive_time(time);
                    }
                    _ => {
                        // not for us - must be destined for another process
                        // on same machine
                    }
                }
            }
            Some(PacketKind::Audio(packet)) => {
                let mut state = state.lock().unwrap();
                state.recv.receive_audio(packet);
            }
            Some(PacketKind::StatsRequest(_)) => {
                let state = state.lock().unwrap();
                let sid = state.recv.current_session().unwrap_or(SessionId::zeroed());
                let receiver = *state.recv.stats();
                drop(state);

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
