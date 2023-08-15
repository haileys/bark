use std::collections::VecDeque;
use std::time::Duration;

use crate::protocol::{AudioPacket, self, TimePacket, TimestampMicros};
use crate::time::{Timestamp, SampleDuration, TimestampDelta, ClockDelta};
use crate::status::Status;

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
}

impl Stream {
    pub fn start_from_packet(packet: &AudioPacket) -> Self {
        Stream {
            sid: packet.sid,
            start_pts: Timestamp::from_micros_lossy(packet.pts),
            start_seq: packet.seq,
            adjust: TimestampDelta::zero(),
            sync: false,
        }
    }

    pub fn pts_for_seq(&self, seq: u64) -> Timestamp {
        let seq_delta = seq.checked_sub(self.start_seq).expect("seq < start seq in pts_for_seq");
        let duration = SampleDuration::ONE_PACKET.mul(seq_delta);
        self.start_pts.add(duration).adjust(self.adjust)
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
        let network_latency_usec = (packet.t3.0 - packet.t1.0) / 2;
        let network_latency = Duration::from_micros(network_latency_usec);
        self.status.record_network_latency(network_latency);

        let clock_delta = ClockDelta::from_time_packet(packet);
        self.status.record_clock_delta(clock_delta);

        if let Some(stream) = self.stream.as_mut() {
            stream.adjust = TimestampDelta::from_clock_delta_lossy(clock_delta);
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
                self.status.clear_sync();
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
                    self.status.clear_sync();
                    self.queue.clear();
                }
            }

            true
        } else {
            self.stream = Some(Stream::start_from_packet(packet));
            self.status.clear_sync();
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
            return;
        };

        let request_end_ts = pts.add(SampleDuration::from_buffer_offset(data.len()));

        // sync up to stream if necessary:
        if !stream.sync {
            loop {
                let Some(front) = self.queue.front_mut() else {
                    // nothing at front of queue?
                    data.fill(0f32);
                    return;
                };

                if pts > front.pts {
                    // frame has already begun, we are late
                    let late = pts.duration_since(front.pts);

                    if late >= SampleDuration::ONE_PACKET {
                        // we are late by more than a packet, skip to the next
                        println!("\nlate by more than a packet, pts: {:?}, front pts: {:?}, late: {:?}", pts, front.pts, late);
                        self.queue.pop_front();
                        continue;
                    }

                    // partially consume this packet to sync up
                    front.consumed = late;

                    // we are synced
                    stream.sync = true;
                    self.status.set_sync();
                    break;
                }

                // otherwise we are early
                let early = front.pts.duration_since(pts);

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
                self.status.set_sync();
                break;
            }
        }

        let mut copy_end_ts = None;

        // copy data to out
        while data.len() > 0 {
            let Some(front) = self.queue.front_mut() else {
                println!("\nqueue underrun, stream-side delay too low");
                data.fill(0f32);
                return;
            };

            let buffer = front.as_full_buffer();
            let buffer_offset = front.consumed.as_buffer_offset();
            let buffer_remaining = buffer.len() - buffer_offset;

            let copy_count = std::cmp::min(data.len(), buffer_remaining);
            let buffer_copy_end = buffer_offset + copy_count;

            data[0..copy_count].copy_from_slice(&buffer[buffer_offset..buffer_copy_end]);

            data = &mut data[copy_count..];
            front.consumed = SampleDuration::from_buffer_offset(buffer_copy_end);
            copy_end_ts = Some(front.pts.add(front.consumed));

            // pop packet if fully consumed
            if front.consumed == SampleDuration::ONE_PACKET {
                self.queue.pop_front();
            }
        }

        if let Some(copy_end_ts) = copy_end_ts {
            self.status.record_audio_latency(request_end_ts, copy_end_ts);
        }

        self.status.render();
    }
}
