use std::{thread, time::Duration};

use bark_core::{audio::Frame, receive::{pipeline::Pipeline, queue::PacketQueue, timing::Timing}};
use bark_protocol::{time::{ClockDelta, SampleDuration, Timestamp, TimestampDelta}, types::{stats::receiver::StreamStatus, AudioPacketHeader, SessionId}, FRAMES_PER_PACKET};
use bytemuck::Zeroable;

use crate::{audio::Output, time};

use super::{queue::{self, Disconnected, QueueReceiver, QueueSender}, Aggregate};

pub struct Stream {
    tx: QueueSender,
    sid: SessionId,
}

impl Stream {
    pub fn new(header: &AudioPacketHeader, output: Output) -> Self {
        let queue = PacketQueue::new(header);
        let (tx, rx) = queue::channel(queue);

        let state = StreamState {
            clock_delta: Aggregate::new(),
            queue: rx,
            pipeline: Pipeline::new(header),
            output,
        };

        thread::spawn(move || {
            run_stream(state);
        });

        Stream {
            tx,
            sid: header.sid,
        }
    }
}

struct StreamState {
    clock_delta: Aggregate<ClockDelta>,
    queue: QueueReceiver,
    pipeline: Pipeline,
    output: Output,
}

pub struct StreamStats {
    status: StreamStatus,
    audio_latency: TimestampDelta,
    output_latency: SampleDuration,
}

impl Default for StreamStats {
    fn default() -> Self {
        StreamStats {
            status: StreamStatus::Seek,
            audio_latency: TimestampDelta::zero(),
            output_latency: SampleDuration::zero(),
        }
    }
}

fn run_stream(mut stream: StreamState) {
    let mut stats = StreamStats::default();

    loop {
        // get next packet from queue, or None if missing (packet loss)
        let packet = match stream.queue.recv() {
            Ok(rx) => rx,
            Err(_) => { return; } // disconnected
        };

        // pass packet through decode pipeline
        let mut buffer = [Frame::zeroed(); FRAMES_PER_PACKET * 2];
        let frames = stream.pipeline.process(packet.as_ref(), &mut buffer);
        let buffer = &buffer[0..frames];

        // get current output delay
        let delay = stream.output.delay().unwrap();
        stats.output_latency = delay;

        // calculate presentation timestamp based on output delay
        let pts = time::now();
        let pts = Timestamp::from_micros_lossy(pts);
        let pts = pts.add(delay);

        // calculate stream timing from packet timing info if present
        let header_pts = packet.as_ref()
            .map(|packet| packet.header().pts)
            .map(Timestamp::from_micros_lossy);

        let stream_pts = header_pts
            .and_then(|header_pts| adjust_pts(&stream, header_pts));

        let timing = stream_pts.map(|stream_pts| Timing {
            real: pts,
            play: stream_pts,
        });

        // adjust resampler rate based on stream timing info
        if let Some(timing) = timing {
            stream.pipeline.set_timing(timing);

            if stream.pipeline.slew() {
                stats.status = StreamStatus::Slew;
            } else {
                stats.status = StreamStatus::Sync;
            }

            stats.audio_latency = timing.real.delta(timing.play);
        }

        // send audio to ALSA
        match stream.output.write(buffer) {
            Ok(()) => {}
            Err(e) => {
                log::error!("error playing audio: {e}");
                break;
            }
        }
    }
}

/// Adjust pts from remote time to local time
fn adjust_pts(stream: &StreamState, pts: Timestamp) -> Option<Timestamp> {
    stream.clock_delta.median().map(|delta| {
        pts.adjust(TimestampDelta::from_clock_delta_lossy(delta))
    })
}