use std::sync::{Arc, Mutex};
use std::thread;

use bark_core::{audio::Frame, receive::{pipeline::Pipeline, queue::{AudioPts, PacketQueue}, timing::Timing}};
use bark_protocol::time::{SampleDuration, Timestamp, TimestampDelta};
use bark_protocol::types::stats::receiver::StreamStatus;
use bark_protocol::types::AudioPacketHeader;
use bark_protocol::FRAMES_PER_PACKET;
use bytemuck::Zeroable;

use crate::time;
use crate::receive::output::OutputRef;
use crate::receive::queue::{self, Disconnected, QueueReceiver, QueueSender};

pub struct DecodeStream {
    tx: QueueSender,
    stats: Arc<Mutex<DecodeStats>>,
}

impl DecodeStream {
    pub fn new(header: &AudioPacketHeader, output: OutputRef) -> Self {
        let queue = PacketQueue::new(header);
        let (tx, rx) = queue::channel(queue);

        let state = State {
            queue: rx,
            pipeline: Pipeline::new(header),
            output,
        };

        let stats = Arc::new(Mutex::new(DecodeStats::default()));

        thread::spawn({
            let stats = stats.clone();
            move || run_stream(state, stats)
        });

        DecodeStream {
            tx,
            stats,
        }
    }

    pub fn send(&self, audio: AudioPts) -> Result<usize, Disconnected> {
        self.tx.send(audio)
    }

    pub fn stats(&self) -> DecodeStats {
        self.stats.lock().unwrap().clone()
    }
}

struct State {
    queue: QueueReceiver,
    pipeline: Pipeline,
    output: OutputRef,
}

#[derive(Clone)]
pub struct DecodeStats {
    pub status: StreamStatus,
    pub buffered: SampleDuration,
    pub audio_latency: TimestampDelta,
    pub output_latency: SampleDuration,
}

impl Default for DecodeStats {
    fn default() -> Self {
        DecodeStats {
            status: StreamStatus::Seek,
            buffered: SampleDuration::zero(),
            audio_latency: TimestampDelta::zero(),
            output_latency: SampleDuration::zero(),
        }
    }
}

fn run_stream(mut stream: State, stats_tx: Arc<Mutex<DecodeStats>>) {
    let mut stats = DecodeStats::default();

    loop {
        // get next packet from queue, or None if missing (packet loss)
        let queue_item = match stream.queue.recv() {
            Ok(rx) => rx,
            Err(_) => { return; } // disconnected
        };

        let (packet, stream_pts) = queue_item.as_ref()
            .map(|item| (Some(&item.audio), Some(item.pts)))
            .unwrap_or_default();

        // pass packet through decode pipeline
        let mut buffer = [Frame::zeroed(); FRAMES_PER_PACKET * 2];
        let frames = stream.pipeline.process(packet, &mut buffer);
        let buffer = &buffer[0..frames];

        // lock output
        let Some(output) = stream.output.lock() else {
            // output has been stolen from us, exit thread
            break;
        };

        // get current output delay
        let delay = output.delay().unwrap();
        stats.output_latency = delay;

        // calculate presentation timestamp based on output delay
        let pts = time::now();
        let pts = Timestamp::from_micros_lossy(pts);
        let pts = pts.add(delay);

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

        // update stats
        *stats_tx.lock().unwrap() = stats.clone();

        // send audio to ALSA
        match output.write(buffer) {
            Ok(()) => {}
            Err(e) => {
                log::error!("error playing audio: {e}");
                break;
            }
        }
    }
}
