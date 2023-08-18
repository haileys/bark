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
    pts: Timestamp,
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
    adjust: TimestampDelta,
    sync: bool,
    resampler: Resampler,
    latency0_usec: Aggregate,
    latency1_usec: Aggregate,
}

impl Stream {
    pub fn start_from_packet(packet: &AudioPacket) -> Self {
        let resampler = Resampler::new();

        Stream {
            sid: packet.sid,
            start_pts: Timestamp::from_micros_lossy(packet.pts),
            start_seq: packet.seq,
            adjust: TimestampDelta::zero(),
            sync: false,
            resampler,
            latency0_usec: Aggregate::new(),
            latency1_usec: Aggregate::new(),
        }
    }

    pub fn pts_for_seq(&self, seq: u64) -> Timestamp {
        let seq_delta = seq.checked_sub(self.start_seq).expect("seq < start seq in pts_for_seq");
        let duration = SampleDuration::ONE_PACKET.mul(seq_delta);
        self.start_pts.add(duration).adjust(self.adjust)
    }

    pub fn network_latency(&self) -> Option<Duration> {
        self.latency0_usec.median().map(Duration::from_micros)
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

        let network_latency_usec = rtt_usec / 2;
        stream.latency0_usec.observe(network_latency_usec);
        stream.latency1_usec.observe(stream.latency0_usec.median().unwrap());

        if let Some(latency) = stream.network_latency() {
            self.status.record_network_latency(latency);
        }

        let clock_delta = ClockDelta::from_time_packet(packet);
        self.status.record_clock_delta(clock_delta);
        stream.adjust = TimestampDelta::from_clock_delta_lossy(clock_delta);
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
                        pts: stream.pts_for_seq(seq),
                        consumed: SampleDuration::zero(),
                        packet: None,
                    })
                }
            }
        } else {
            // queue is empty, insert missing packet slot for the packet we are about to receive
            self.queue.push_back(QueueEntry {
                seq: packet.seq,
                pts: stream.pts_for_seq(packet.seq),
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

        let request_end_ts = pts.add(SampleDuration::from_buffer_offset(data.len()));

        // sync up to stream if necessary:
        if !stream.sync {
            loop {
                let Some(front) = self.queue.front_mut() else {
                    // nothing at front of queue?
                    data.fill(0f32);
                    self.status.render();
                    return;
                };

                if pts > front.pts {
                    // frame has already begun, we are late
                    let late = pts.duration_since(front.pts);

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
                let early = front.pts.duration_since(pts);

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

        let mut copy_end_ts = None;

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

            copy_end_ts = Some(front.pts.add(front.consumed));

            // pop packet if fully consumed
            if front.consumed == SampleDuration::ONE_PACKET {
                self.queue.pop_front();
            }
        }

        if let Some(copy_end_ts) = copy_end_ts {
            if let Some(rate) = adjusted_playback_rate(request_end_ts, copy_end_ts) {
                let _ = stream.resampler.set_input_rate(rate);
                self.status.set_stream(StreamStatus::Slew);
            } else {
                let _ = stream.resampler.set_input_rate(protocol::SAMPLE_RATE.0);
                self.status.set_stream(StreamStatus::Sync);
            }

            self.status.record_audio_latency(request_end_ts, copy_end_ts);
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

struct Aggregate {
    samples: [u64; 64],
    count: usize,
    index: usize,
}

impl Aggregate {
    pub fn new() -> Self {
        Aggregate { samples: [0u64; 64], count: 0, index: 0 }
    }

    pub fn observe(&mut self, value: u64) {
        self.samples[self.index] = value;

        if self.count < self.samples.len() {
            self.count += 1;
        }

        self.index += 1;
        self.index %= self.samples.len();
    }

    pub fn median(&self) -> Option<u64> {
        let mut samples = self.samples;
        let samples = &mut samples[0..self.count];
        samples.sort();
        samples.get(self.count / 2).copied()
    }
}
