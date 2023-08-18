use std::array;
use std::collections::VecDeque;
use std::time::Duration;

use crate::protocol::{AudioPacket, self, TimePacket, TimestampMicros};
use crate::time::{Timestamp, SampleDuration, TimestampDelta, ClockDelta};
use crate::status::{Status, StreamStatus};
use crate::resample::Resampler;

pub struct Receiver {
    opt: ReceiverOpt,
    status: Status,
    stream: Option<Stream>,
    queue: VecDeque<QueueEntry>,
}

pub struct ReceiverOpt {
    pub max_seq_gap: usize,
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
    sid: TimestampMicros,
    start_pts: Timestamp,
    start_seq: u64,
    sync: bool,
    resampler: Resampler,
    latency: Aggregate<Duration>,
    clock_delta: Aggregate<ClockDelta>,
}

impl Stream {
    pub fn start_from_packet(packet: &AudioPacket) -> Self {
        let resampler = Resampler::new();

        Stream {
            sid: packet.sid,
            start_pts: Timestamp::from_micros_lossy(packet.pts),
            start_seq: packet.seq,
            sync: false,
            resampler,
            latency: Aggregate::new(),
            clock_delta: Aggregate::new(),
        }
    }

    pub fn adjust_pts(&self, pts: Timestamp) -> Option<Timestamp> {
        self.clock_delta.median().map(|delta| {
            pts.adjust(TimestampDelta::from_clock_delta_lossy(delta))
        })
    }

    // pub fn expected_pts_for_seq(&self, seq: u64) -> Timestamp {
    //     let network_latency = self.network_latency();
    //     let clock_delta = self.clock_delta();

    //     let seq_since_start = seq.checked_sub(self.start_seq)
    //         .expect("seq < start seq in pts_for_seq");

    //     let duration_since_start = SampleDuration::ONE_PACKET.mul(seq_since_start);

    //     self.start_pts.add(duration_since_start).adjust(self.adjust)
    // }

    pub fn network_latency(&self) -> Option<Duration> {
        self.latency.median()
    }

    pub fn clock_delta(&self) -> Option<ClockDelta> {
        self.clock_delta.median()
    }
}

#[derive(Clone, Copy)]
pub struct ClockInfo {
    pub network_latency_usec: i64,
    pub clock_diff_usec: i64,
}

impl Receiver {
    pub fn new(opt: ReceiverOpt) -> Self {
        let queue = VecDeque::with_capacity(opt.max_seq_gap);

        Receiver {
            opt,
            stream: None,
            queue,
            status: Status::new(),
        }
    }

    pub fn receive_time(&mut self, packet: &TimePacket) {
        let Some(stream) = self.stream.as_mut() else {
            // no stream, nothing we can do with a time packet
            return;
        };

        if stream.sid.0 != packet.sid.0 {
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
            self.status.record_network_latency(latency);
        }

        let clock_delta = ClockDelta::from_time_packet(packet);
        stream.clock_delta.observe(clock_delta);

        if let Some(delta) = stream.clock_delta.median() {
            self.status.record_clock_delta(delta);
        }
    }

    fn prepare_stream(&mut self, packet: &AudioPacket) -> bool {
        if let Some(stream) = self.stream.as_mut() {
            if packet.sid.0 < stream.sid.0 {
                // packet belongs to a previous stream, ignore
                return false;
            }

            if packet.sid.0 > stream.sid.0 {
                // new stream is taking over! switch over to it
                println!("\nnew stream beginning");
                self.stream = Some(Stream::start_from_packet(packet));
                self.status.clear_stream();
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
                    self.status.clear_stream();
                    self.queue.clear();
                }
            }

            true
        } else {
            self.stream = Some(Stream::start_from_packet(packet));
            self.status.clear_stream();
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
                self.status.record_dts_prediction_difference(predict_diff)
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
            self.status.render();
            return;
        };

        let real_ts_after_fill = pts.add(SampleDuration::from_buffer_offset(data.len()));

        // sync up to stream if necessary:
        if !stream.sync {
            loop {
                let Some(front) = self.queue.front_mut() else {
                    // nothing at front of queue?
                    data.fill(0f32);
                    self.status.render();
                    return;
                };

                let Some(front_pts) = front.pts else {
                    // haven't received enough info to adjust pts of queue
                    // front yet, just pop and ignore it
                    self.queue.pop_front();
                    // and output silence for this part:
                    data.fill(0f32);
                    self.status.render();
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
                    self.status.set_stream(StreamStatus::Sync);
                    break;
                }

                // otherwise we are early
                let early = front_pts.duration_since(pts);

                if early >= SampleDuration::from_buffer_offset(data.len()) {
                    // we are early by more than what was asked of us in this
                    // call, fill with zeroes and return
                    data.fill(0f32);
                    self.status.render();
                    return;
                }

                // we are early, but not an entire packet timing's early
                // partially output some zeroes
                let zero_count = early.as_buffer_offset();
                data[0..zero_count].fill(0f32);
                data = &mut data[zero_count..];

                // then mark ourselves as synced and fall through to regular processing
                stream.sync = true;
                self.status.set_stream(StreamStatus::Sync);
                break;
            }
        }

        let mut stream_ts = None;

        // copy data to out
        while data.len() > 0 {
            let Some(front) = self.queue.front_mut() else {
                data.fill(0f32);
                self.status.set_stream(StreamStatus::Miss);
                self.status.render();
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
            if let Some(rate) = adjusted_playback_rate(real_ts_after_fill, stream_ts) {
                let _ = stream.resampler.set_input_rate(rate);
                self.status.set_stream(StreamStatus::Slew);
            } else {
                let _ = stream.resampler.set_input_rate(protocol::SAMPLE_RATE.0);
                self.status.set_stream(StreamStatus::Sync);
            }

            self.status.record_audio_latency(real_ts_after_fill, stream_ts);
        }

        self.status.record_buffer_length(self.queue.iter()
            .map(|entry| SampleDuration::ONE_PACKET.sub(entry.consumed))
            .fold(SampleDuration::zero(), |cum, dur| cum.add(dur)));

        self.status.render();
    }
}

fn adjusted_playback_rate(real_ts: Timestamp, play_ts: Timestamp) -> Option<u32> {
    let delta = real_ts.delta(play_ts).as_frames();
    let one_sec = i64::from(protocol::SAMPLE_RATE.0);
    let one_ms = one_sec / 1000;

    if delta.abs() > one_sec {
        // we should desync here
    }

    if delta.abs() < one_ms {
        // no need to adjust
        return None;
    }

    if delta > 0 {
        // real_ts > play_ts, ie. we are running slow
        // speed up playback rate by 1%
        let rate = protocol::SAMPLE_RATE.0 * 101 / 100;
        return Some(rate);
    } else {
        // real_ts < play_ts, ie. we are running fast
        // speed up playback rate by 1%
        let rate = protocol::SAMPLE_RATE.0 * 99 / 100;
        return Some(rate);
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
