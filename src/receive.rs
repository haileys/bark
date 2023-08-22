use std::array;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytemuck::Zeroable;
use cpal::{SampleRate, OutputCallbackInfo};
use cpal::traits::{HostTrait, DeviceTrait};
use structopt::StructOpt;

use crate::protocol::{AudioPacket, self, TimePacket, TimestampMicros, Packet, SessionId, ReceiverId, TimePhase, StatsReplyPacket, StatsReplyFlags};
use crate::resample::Resampler;
use crate::socket::{Socket, SocketOpt};
use crate::stats::node::NodeStats;
use crate::stats::receiver::{ReceiverStats, StreamStatus};
use crate::time::{Timestamp, SampleDuration, TimestampDelta, ClockDelta};
use crate::util;
use crate::RunError;

pub struct Receiver {
    opt: ReceiveOpt,
    stats: ReceiverStats,
    stream: Option<Stream>,
    queue: VecDeque<QueueEntry>,
}

struct QueueEntry {
    seq: u64,
    pts: Option<Timestamp>,
    consumed: SampleDuration,
    packet: Option<AudioPacket>,
}

impl QueueEntry {
    pub fn as_full_buffer(&self) -> &[f32; protocol::SAMPLES_PER_PACKET] {
        self.packet.as_ref()
            .map(|packet| &packet.buffer.0)
            .unwrap_or(&[0f32; protocol::SAMPLES_PER_PACKET])
    }
}

struct Stream {
    sid: SessionId,
    start_seq: u64,
    sync: bool,
    resampler: Resampler,
    rate_adjust: RateAdjust,
    latency: Aggregate<Duration>,
    clock_delta: Aggregate<ClockDelta>,
}

impl Stream {
    pub fn start_from_packet(packet: &AudioPacket) -> Self {
        let resampler = Resampler::new();

        Stream {
            sid: packet.sid,
            start_seq: packet.seq,
            sync: false,
            resampler,
            rate_adjust: RateAdjust::new(),
            latency: Aggregate::new(),
            clock_delta: Aggregate::new(),
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
    pub fn new(opt: ReceiveOpt) -> Self {
        let queue = VecDeque::with_capacity(opt.max_seq_gap);

        Receiver {
            opt,
            stream: None,
            queue,
            stats: ReceiverStats::new(),
        }
    }

    pub fn stats(&self) -> &ReceiverStats {
        &self.stats
    }

    pub fn current_session(&self) -> Option<SessionId> {
        self.stream.as_ref().map(|s| s.sid)
    }

    pub fn receive_time(&mut self, packet: &TimePacket) {
        let Some(stream) = self.stream.as_mut() else {
            // no stream, nothing we can do with a time packet
            return;
        };

        if stream.sid != packet.sid {
            // not relevant to our stream, ignore
            return;
        }

        let stream_1_usec = packet.stream_1.0;
        let stream_3_usec = packet.stream_3.0;

        let Some(rtt_usec) = stream_3_usec.checked_sub(stream_1_usec) else {
            // invalid packet, ignore
            return;
        };

        let network_latency = Duration::from_micros(rtt_usec / 2);
        stream.latency.observe(network_latency);

        if let Some(latency) = stream.network_latency() {
            self.stats.set_network_latency(latency);
        }

        let clock_delta = ClockDelta::from_time_packet(packet);
        stream.clock_delta.observe(clock_delta);
    }

    fn prepare_stream(&mut self, packet: &AudioPacket) -> bool {
        if let Some(stream) = self.stream.as_mut() {
            if packet.sid < stream.sid {
                // packet belongs to a previous stream, ignore
                return false;
            }

            if packet.sid > stream.sid {
                // new stream is taking over! switch over to it
                println!("\nnew stream beginning");
                self.stream = Some(Stream::start_from_packet(packet));
                self.stats.clear();
                self.queue.clear();
                return true;
            }

            if packet.seq < stream.start_seq {
                println!("\nreceived packet with seq before start, dropping");
                return false;
            }

            if let Some(front) = self.queue.front() {
                if packet.seq <= front.seq {
                    println!("\nreceived packet with seq <= queue front seq, dropping");
                    return false;
                }
            }

            if let Some(back) = self.queue.back() {
                if back.seq + self.opt.max_seq_gap as u64 <= packet.seq {
                    println!("\nreceived packet with seq too far in future, resetting stream");
                    self.stream = Some(Stream::start_from_packet(packet));
                    self.stats.clear();
                    self.queue.clear();
                }
            }

            true
        } else {
            self.stream = Some(Stream::start_from_packet(packet));
            self.stats.clear();
            true
        }
    }

    pub fn receive_audio(&mut self, packet: &AudioPacket) {
        let now = TimestampMicros::now();

        if packet.flags != 0 {
            println!("\nunknown flags in packet, ignoring entire packet");
            return;
        }

        if !self.prepare_stream(packet) {
            return;
        }

        // we are guaranteed that if prepare_stream returns true,
        // self.stream is Some:
        let stream = self.stream.as_ref().unwrap();

        if let Some(latency) = stream.network_latency() {
            if let Some(clock_delta) = stream.clock_delta.median() {
                let latency_usec = u64::try_from(latency.as_micros()).unwrap();
                let delta_usec = clock_delta.as_micros();
                let predict_dts = (now.0 - latency_usec).checked_add_signed(-delta_usec).unwrap();
                let predict_diff = predict_dts as i64 - packet.dts.0 as i64;
                self.stats.set_predict_offset(predict_diff)
            }
        }

        // INVARIANT: at this point we are guaranteed that, if there are
        // packets in the queue, the seq of the incoming packet is less than
        // back.seq + max_seq_gap

        // expand queue to make space for new packet
        if let Some(back) = self.queue.back() {
            if packet.seq > back.seq {
                // extend queue from back to make space for new packet
                // this also allows for out of order packets
                for seq in (back.seq + 1)..=packet.seq {
                    self.queue.push_back(QueueEntry {
                        seq,
                        pts: None,
                        consumed: SampleDuration::zero(),
                        packet: None,
                    })
                }
            }
        } else {
            // queue is empty, insert missing packet slot for the packet we are about to receive
            self.queue.push_back(QueueEntry {
                seq: packet.seq,
                pts: None,
                consumed: SampleDuration::zero(),
                packet: None,
            });
        }

        // INVARIANT: at this point queue is non-empty and contains an
        // allocated slot for the packet we just received
        let front_seq = self.queue.front().unwrap().seq;
        let idx_for_packet = (packet.seq - front_seq) as usize;

        let slot = self.queue.get_mut(idx_for_packet).unwrap();
        assert!(slot.seq == packet.seq);
        slot.packet = Some(*packet);
        slot.pts = stream.adjust_pts(Timestamp::from_micros_lossy(packet.pts))
    }

    pub fn fill_stream_buffer(&mut self, mut data: &mut [f32], pts: Timestamp) {
        // complete frames only:
        assert!(data.len() % 2 == 0);

        // get stream start timing information:
        let Some(stream) = self.stream.as_mut() else {
            // stream hasn't started, just fill buffer with silence and return
            data.fill(0f32);
            return;
        };

        let real_ts_after_fill = pts.add(SampleDuration::from_buffer_offset(data.len()));

        // sync up to stream if necessary:
        if !stream.sync {
            loop {
                let Some(front) = self.queue.front_mut() else {
                    // nothing at front of queue?
                    data.fill(0f32);
                    return;
                };

                let Some(front_pts) = front.pts else {
                    // haven't received enough info to adjust pts of queue
                    // front yet, just pop and ignore it
                    self.queue.pop_front();
                    // and output silence for this part:
                    data.fill(0f32);
                    return;
                };

                if pts > front_pts {
                    // frame has already begun, we are late
                    let late = pts.duration_since(front_pts);

                    if late >= SampleDuration::ONE_PACKET {
                        // we are late by more than a packet, skip to the next
                        self.queue.pop_front();
                        continue;
                    }

                    // partially consume this packet to sync up
                    front.consumed = late;

                    // we are synced
                    stream.sync = true;
                    self.stats.set_stream(StreamStatus::Sync);
                    break;
                }

                // otherwise we are early
                let early = front_pts.duration_since(pts);

                if early >= SampleDuration::from_buffer_offset(data.len()) {
                    // we are early by more than what was asked of us in this
                    // call, fill with zeroes and return
                    data.fill(0f32);
                    return;
                }

                // we are early, but not an entire packet timing's early
                // partially output some zeroes
                let zero_count = early.as_buffer_offset();
                data[0..zero_count].fill(0f32);
                data = &mut data[zero_count..];

                // then mark ourselves as synced and fall through to regular processing
                stream.sync = true;
                self.stats.set_stream(StreamStatus::Sync);
                break;
            }
        }

        let mut stream_ts = None;

        // copy data to out
        while data.len() > 0 {
            let Some(front) = self.queue.front_mut() else {
                data.fill(0f32);
                self.stats.set_stream(StreamStatus::Miss);
                return;
            };

            let buffer = front.as_full_buffer();
            let buffer_offset = front.consumed.as_buffer_offset();
            let buffer_remaining = buffer.len() - buffer_offset;

            let copy_count = std::cmp::min(data.len(), buffer_remaining);
            let buffer_copy_end = buffer_offset + copy_count;

            let input = &buffer[buffer_offset..buffer_copy_end];
            let output = &mut data[0..copy_count];
            let result = stream.resampler.process_interleaved(input, output)
                .expect("resample error!");

            data = &mut data[result.output_written.as_buffer_offset()..];
            front.consumed = front.consumed.add(result.input_read);

            stream_ts = front.pts.map(|front_pts| front_pts.add(front.consumed));

            // pop packet if fully consumed
            if front.consumed == SampleDuration::ONE_PACKET {
                self.queue.pop_front();
            }
        }

        if let Some(stream_ts) = stream_ts {
            let rate = stream.rate_adjust.sample_rate(Timing {
                real: real_ts_after_fill,
                play: stream_ts,
            });

            let _ = stream.resampler.set_input_rate(rate.0);

            if stream.rate_adjust.slew() {
                self.stats.set_stream(StreamStatus::Slew);
            } else {
                self.stats.set_stream(StreamStatus::Sync);
            }

            self.stats.set_audio_latency(real_ts_after_fill, stream_ts);
        }

        self.stats.set_buffer_length(self.queue.iter()
            .map(|entry| SampleDuration::ONE_PACKET.sub(entry.consumed))
            .fold(SampleDuration::zero(), |cum, dur| cum.add(dur)));
    }
}

struct RateAdjust {
    slew: bool,
}

#[derive(Copy, Clone)]
pub struct Timing {
    pub real: Timestamp,
    pub play: Timestamp,
}

impl RateAdjust {
    pub fn new() -> Self {
        RateAdjust {
            slew: false
        }
    }

    pub fn slew(&self) -> bool {
        self.slew
    }

    pub fn sample_rate(&mut self, timing: Timing) -> SampleRate {
        self.adjusted_rate(timing).unwrap_or(protocol::SAMPLE_RATE)
    }

    fn adjusted_rate(&mut self, timing: Timing) -> Option<SampleRate> {
        // parameters, maybe these could be cli args?
        let start_slew_threshold = Duration::from_micros(2000);
        let stop_slew_threshold = Duration::from_micros(100);
        let slew_target_duration = Duration::from_millis(500);

        // turn them into native units
        let start_slew_threshold = SampleDuration::from_std_duration_lossy(start_slew_threshold);
        let stop_slew_threshold = SampleDuration::from_std_duration_lossy(stop_slew_threshold);

        let frame_offset = timing.real.delta(timing.play);

        if frame_offset.abs() < stop_slew_threshold {
            self.slew = false;
            return None;
        }

        if frame_offset.abs() < start_slew_threshold && !self.slew {
            return None;
        }

        let slew_duration_duration = i64::try_from(slew_target_duration.as_micros()).unwrap();
        let base_sample_rate = i64::from(protocol::SAMPLE_RATE.0);
        let rate_offset = frame_offset.as_frames() * 1_000_000 / slew_duration_duration;
        let rate = base_sample_rate + rate_offset;

        // clamp any potential slow down to 2%, we shouldn't ever get too far
        // ahead of the stream
        let rate = std::cmp::max(base_sample_rate * 98 / 100, rate);

        // let the speed up run much higher, but keep it reasonable still
        let rate = std::cmp::min(base_sample_rate * 2, rate);

        self.slew = true;
        Some(SampleRate(u32::try_from(rate).unwrap()))
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
    #[structopt(long, env = "BARK_RECEIVE_DEVICE")]
    pub device: Option<String>,
    #[structopt(long, default_value="12")]
    pub max_seq_gap: usize,
}

pub fn run(opt: ReceiveOpt) -> Result<(), RunError> {
    let receiver_id = ReceiverId::generate();
    let node = NodeStats::get();

    if let Some(device) = &opt.device {
        crate::audio::set_sink_env(device);
    }

    let host = cpal::default_host();

    let device = host.default_output_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = util::config_for_device(&device)?;

    struct SharedState {
        pub recv: Receiver,
    }

    let state = Arc::new(Mutex::new(SharedState {
        recv: Receiver::new(opt.clone()),
    }));

    let _stream = device.build_output_stream(&config,
        {
            let state = state.clone();
            let mut initialized_thread = false;
            move |data: &mut [f32], info: &OutputCallbackInfo| {
                if !initialized_thread {
                    crate::thread::set_name("bark/audio");
                    crate::thread::set_realtime_priority();
                    initialized_thread = true;
                }

                let stream_timestamp = info.timestamp();

                let output_latency = stream_timestamp.playback
                    .duration_since(&stream_timestamp.callback)
                    .unwrap_or_default();

                let output_latency = SampleDuration::from_std_duration_lossy(output_latency);

                let now = Timestamp::now();
                let pts = now.add(output_latency);

                let mut state = state.lock().unwrap();
                state.recv.fill_stream_buffer(data, pts);
            }
        },
        move |err| {
            eprintln!("stream error! {err:?}");
        },
        None
    ).map_err(RunError::BuildStream)?;

    let socket = Socket::open(opt.socket)
        .map_err(RunError::Listen)?;

    crate::thread::set_name("bark/network");
    crate::thread::set_realtime_priority();

    loop {
        let mut packet_raw = [0u8; protocol::MAX_PACKET_SIZE];

        let (nbytes, addr) = socket.recv_from(&mut packet_raw)
            .map_err(RunError::Socket)?;

        match Packet::try_from_bytes_mut(&mut packet_raw[0..nbytes]) {
            Some(Packet::Time(time)) => {
                if !time.rid.matches(&receiver_id) {
                    // not for us - time packets are usually unicast,
                    // but there can be multiple receivers on a machine
                    continue;
                }

                match time.phase() {
                    Some(TimePhase::Broadcast) => {
                        time.receive_2 = TimestampMicros::now();
                        time.rid = receiver_id;
                        socket.send_to(bytemuck::bytes_of(time), addr)
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
            Some(Packet::Audio(packet)) => {
                let mut state = state.lock().unwrap();
                state.recv.receive_audio(packet);
            }
            Some(Packet::StatsRequest(_)) => {
                let state = state.lock().unwrap();
                let sid = state.recv.current_session();
                let stats = *state.recv.stats();
                drop(state);

                let reply = StatsReplyPacket {
                    magic: protocol::MAGIC_STATS_REPLY,
                    flags: StatsReplyFlags::IS_RECEIVER,
                    sid: sid.unwrap_or(SessionId::zeroed()),
                    receiver: stats,
                    node,
                };

                let _ = socket.send_to(bytemuck::bytes_of(&reply), addr);
            }
            Some(Packet::StatsReply(_)) => {
                // ignore
            }
            None => {
                // unknown packet type, ignore
            }
        }
    }
}
