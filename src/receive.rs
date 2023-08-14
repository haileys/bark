use std::collections::VecDeque;

use crate::protocol::{Packet, RECEIVER_BUFFERED_PACKETS, self};
use crate::time::{Timestamp, SampleDuration};

pub struct Receiver {
    start: Option<StreamStart>,
    queue: VecDeque<QueueEntry>,
}

struct QueueEntry {
    seq: u64,
    pts: Timestamp,
    consumed: SampleDuration,
    packet: Option<Packet>,
}

impl QueueEntry {
    pub fn as_full_buffer(&self) -> &[f32; protocol::SAMPLES_PER_PACKET] {
        self.packet.as_ref()
            .map(|packet| &packet.buffer.0)
            .unwrap_or(&[0f32; protocol::SAMPLES_PER_PACKET])
    }
}

struct StreamStart {
    pts: Timestamp,
    seq: u64,
    sync: bool,
}

impl StreamStart {
    pub fn from_packet(packet: &Packet) -> Self {
        StreamStart {
            pts: Timestamp::from_micros_lossy(packet.pts),
            seq: packet.seq,
            sync: false,
        }
    }

    pub fn pts_for_seq(&self, seq: u64) -> Timestamp {
        let delta = seq.checked_sub(self.seq).expect("seq < start seq in pts_for_seq");
        let duration = SampleDuration::ONE_PACKET.mul(delta);
        self.pts.add(duration)
    }
}

pub enum PacketDisposition {
    Pop,
    Pass,
}

impl Receiver {
    pub fn new() -> Self {
        Receiver {
            start: None,
            queue: VecDeque::new(),
        }
    }

    pub fn push_packet(&mut self, packet: &Packet) {
        if let Some(start) = self.start.as_mut() {
            if packet.seq < start.seq {
                eprintln!("received packet with seq before start, dropping");
                return;
            }

            if let Some(front) = self.queue.front() {
                if packet.seq <= front.seq {
                    eprintln!("received packet with seq <= queue front seq, dropping");
                    return;
                }
            }

            if let Some(back) = self.queue.back() {
                if back.seq + RECEIVER_BUFFERED_PACKETS as u64 <= packet.seq {
                    eprintln!("received packet with seq too far in future, resetting stream");
                    self.start = Some(StreamStart::from_packet(packet));
                    self.queue.clear();
                }
            }
        } else {
            self.start = Some(StreamStart::from_packet(packet));
        }

        let start = self.start.as_ref().unwrap();

        // INVARIANT: at this point we are guaranteed that, if there are
        // packets in the queue, the seq of the incoming packet is less than
        // back.seq + RECEIVER_BUFFERED_PACKETS

        // expand queue to make space for new packet
        if let Some(back) = self.queue.back() {
            if packet.seq > back.seq {
                // extend queue from back to make space for new packet
                // this also allows for out of order packets
                for seq in (back.seq + 1)..=packet.seq {
                    self.queue.push_back(QueueEntry {
                        seq,
                        pts: start.pts_for_seq(seq),
                        consumed: SampleDuration::zero(),
                        packet: None,
                    })
                }
            }
        } else {
            // queue is empty, insert missing packet slot for the packet we are about to receive
            self.queue.push_back(QueueEntry {
                seq: packet.seq,
                pts: start.pts_for_seq(packet.seq),
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
        let Some(start) = self.start.as_mut() else {
            // stream hasn't started, just fill buffer with silence and return
            data.fill(0f32);
            return;
        };

        // sync up to stream if necessary:
        if !start.sync {
            loop {
                let Some(front) = self.queue.front_mut() else {
                    // nothing at front of queue?
                    data.fill(0f32);
                    return;
                };

                eprintln!("something at front of queue!");

                if pts > front.pts {
                    // frame has already begun, we are late
                    let late = pts.duration_since(front.pts);

                    if late >= SampleDuration::ONE_PACKET {
                        // we are late by more than a packet, skip to the next
                        eprintln!("late by more than a packet, pts: {:?}, front pts: {:?}, late: {:?}", pts, front.pts, late);
                        self.queue.pop_front();
                        continue;
                    }

                    // partially consume this packet to sync up
                    front.consumed = late;

                    // we are synced
                    println!("SYNC!");
                    start.sync = true;
                    break;
                }

                // otherwise we are early
                let early = front.pts.duration_since(pts);

                if early >= SampleDuration::ONE_PACKET {
                    // we are early by more than a packet, fill buffer with silence and return
                    eprintln!("early by more than a packet");
                    data.fill(0f32);
                    return;
                }

                // we are early, but not an entire packet timing's early
                // partially output some zeroes
                let zero_count = early.as_buffer_offset();
                data[0..zero_count].fill(0f32);
                data = &mut data[zero_count..];

                // then mark ourselves as synced and fall through to regular processing
                println!("SYNC!");
                start.sync = true;
                break;
            }
        }

        // copy data to out
        while data.len() > 0 {
            let Some(front) = self.queue.front_mut() else {
                eprintln!("nothing at the front of the queue!");
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

            // pop packet if fully consumed
            if front.consumed == SampleDuration::ONE_PACKET {
                self.queue.pop_front();
            }
        }
    }
}
