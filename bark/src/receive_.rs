use std::array;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bark_protocol::types::stats::node::NodeStats;
use bytemuck::Zeroable;
use cpal::OutputCallbackInfo;
use cpal::traits::{HostTrait, DeviceTrait};
use structopt::StructOpt;

use bark_protocol::{SampleRate, buffer};
use bark_protocol::time::{Timestamp, SampleDuration, TimestampDelta, ClockDelta};
use bark_protocol::types::{SessionId, ReceiverId, TimePhase};
use bark_protocol::types::stats::receiver::{ReceiverStats, StreamStatus};
use bark_protocol::packet::{Audio, Time, PacketKind, StatsReply};
use bark_network::{ProtocolSocket, Socket};

use crate::resample::Resampler;
use crate::stats;
use crate::{RunError, SocketOpt};

mod consts;
mod queue;
mod stream;

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
    packet: Option<Audio>,
}

impl QueueEntry {
    pub fn as_full_buffer(&self) -> &[f32] {
        self.packet.as_ref()
            .map(|packet| packet.buffer())
            .unwrap_or(&[0f32; bark_protocol::SAMPLES_PER_PACKET])
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
    pub fn start_from_packet(audio: &Audio) -> Self {
        let resampler = Resampler::new();

        Stream {
            sid: audio.header().sid,
            start_seq: audio.header().seq,
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

    fn prepare_stream(&mut self, packet: &Audio) -> bool {
        if let Some(stream) = self.stream.as_mut() {
            let header = packet.header();

            if header.sid < stream.sid {
                // packet belongs to a previous stream, ignore
                return false;
            }

            if header.sid > stream.sid {
                // new stream is taking over! switch over to it
                println!("\nnew stream beginning");
                self.stream = Some(Stream::start_from_packet(packet));
                self.stats.clear();
                self.queue.clear();
                return true;
            }

            if header.seq < stream.start_seq {
                println!("\nreceived packet with seq before start, dropping");
                return false;
            }

            if let Some(front) = self.queue.front() {
                if header.seq <= front.seq {
                    println!("\nreceived packet with seq <= queue front seq, dropping");
                    return false;
                }
            }

            if let Some(back) = self.queue.back() {
                if back.seq + self.opt.max_seq_gap as u64 <= header.seq {
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

    pub fn receive_audio(&mut self, packet: Audio) {
        let now = bark_util::time::now();

        if !self.prepare_stream(&packet) {
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
                let predict_diff = predict_dts as i64 - packet.header().dts.0 as i64;
                self.stats.set_predict_offset(predict_diff)
            }
        }

        // INVARIANT: at this point we are guaranteed that, if there are
        // packets in the queue, the seq of the incoming packet is less than
        // back.seq + max_seq_gap

        // expand queue to make space for new packet
        if let Some(back) = self.queue.back() {
            if packet.header().seq > back.seq {
                // extend queue from back to make space for new packet
                // this also allows for out of order packets
                for seq in (back.seq + 1)..=packet.header().seq {
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
                seq: packet.header().seq,
                pts: None,
                consumed: SampleDuration::zero(),
                packet: None,
            });
        }

        // INVARIANT: at this point queue is non-empty and contains an
        // allocated slot for the packet we just received
        let front_seq = self.queue.front().unwrap().seq;
        let idx_for_packet = (packet.header().seq - front_seq) as usize;

        let slot = self.queue.get_mut(idx_for_packet).unwrap();
        assert!(slot.seq == packet.header().seq);
        slot.pts = stream.adjust_pts(Timestamp::from_micros_lossy(packet.header().pts));
        slot.packet = Some(packet);
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

pub fn run(opt: ReceiveOpt) -> Result<(), RunError> {


    let receiver = Arc::new(Mutex::new(Receiver::new(opt.clone())));

    std::thread::spawn({
        move || {
            network_thread(protocol, receiver_id, receiver, node);
        }
    });

    // loop {

    // }
    let _stream = device.build_output_stream(&config,
        {
            let state = state.clone();
            let mut initialized_thread = false;
            move |data: &mut [f32], info: &OutputCallbackInfo| {
                if !initialized_thread {
                    bark_util::thread::set_name("bark/audio");
                    bark_util::thread::set_realtime_priority();
                    initialized_thread = true;
                }

                let stream_timestamp = info.timestamp();

                let output_latency = stream_timestamp.playback
                    .duration_since(&stream_timestamp.callback)
                    .unwrap_or_default();

                let output_latency = SampleDuration::from_std_duration_lossy(output_latency);

                let now = Timestamp::from_micros_lossy(bark_util::time::now());
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

}

fn network_thread(
    protocol: ProtocolSocket,
    receiver_id: ReceiverId,
    receiver: Arc<Mutex<Receiver>>,
    node: NodeStats,
) {
    bark_util::thread::set_name("bark/network");
    bark_util::thread::set_realtime_priority();

    loop {
        let (packet, peer) = protocol.recv_from().map_err(RunError::Socket)?;

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
                        data.receive_2 = bark_util::time::now();
                        data.rid = receiver_id;

                        protocol.send_to(time.as_packet(), peer)
                            .expect("reply to time packet");
                    }
                    Some(TimePhase::StreamReply) => {
                        let mut recv = receiver.lock().unwrap();
                        recv.receive_time(time);
                    }
                    _ => {
                        // not for us - must be destined for another process
                        // on same machine
                    }
                }
            }
            Some(PacketKind::Audio(packet)) => {
                let mut recv = receiver.lock().unwrap();
                recv.receive_audio(packet);
            }
            Some(PacketKind::StatsRequest(_)) => {
                let recv = receiver.lock().unwrap();
                let sid = recv.current_session().unwrap_or(SessionId::zeroed());
                let receiver = *recv.stats();
                drop(recv);

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
