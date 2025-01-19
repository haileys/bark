use std::sync::{Arc, Mutex};

use bark_core::audio::{Format, FrameCount};
use bark_core::receive::pipeline::Pipeline;
use bark_core::receive::queue::{AudioPts, PacketQueue};
use bark_core::receive::timing::Timing;
use bark_protocol::time::{SampleDuration, Timestamp, TimestampDelta};
use bark_protocol::types::stats::receiver::StreamStatus;
use bark_protocol::types::AudioPacketHeader;
use bark_protocol::FRAMES_PER_PACKET;
use bytemuck::Zeroable;

use crate::stats::server::MetricsSender;
use crate::time;
use crate::receive::output::OutputRef;
use crate::receive::queue::{self, Disconnected, QueueReceiver, QueueSender};
use crate::thread;

pub struct DecodeStream {
    tx: QueueSender,
    stats: Arc<Mutex<DecodeStats>>,
}

impl DecodeStream {
    pub fn new<F: Format>(header: &AudioPacketHeader, output: OutputRef<F>, metrics: MetricsSender) -> Self {
        let queue = PacketQueue::new(header);
        let (tx, rx) = queue::channel(queue);

        let state = State {
            queue: rx,
            pipeline: Pipeline::new(header),
            output,
            metrics,
        };

        let stats = Arc::new(Mutex::new(DecodeStats::default()));

        std::thread::spawn({
            let stats = stats.clone();
            move || {
                thread::set_name("bark/audio");
                thread::set_realtime_priority();
                run_stream(state, stats);
            }
        });

        DecodeStream {
            tx,
            stats,
        }
    }

    pub fn send(&self, audio: AudioPts) -> Result<(), Disconnected> {
        self.tx.send(audio)
    }

    pub fn stats(&self) -> DecodeStats {
        self.stats.lock().unwrap().clone()
    }
}

struct State<F: Format> {
    queue: QueueReceiver,
    pipeline: Pipeline<F>,
    output: OutputRef<F>,
    metrics: MetricsSender,
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

fn run_stream<F: Format>(mut stream: State<F>, stats_tx: Arc<Mutex<DecodeStats>>) {
    let mut stats = DecodeStats::default();

    loop {
        // get next packet from queue, or None if missing (packet loss)
        let (queue_item, queue_len) = match stream.queue.recv() {
            Ok(rx) => rx,
            Err(_) => { return; } // disconnected
        };

        // update queue related metrics
        stream.metrics.observe_queued_packets(queue_len);

        if queue_item.is_none() {
            if queue_len == 0 {
                // if packet is missing because the queue is empty, we are running too
                // hot up against the stream and missed our deadline
                stream.metrics.increment_packets_missed();
            } else {
                // if the queue is not empty, this is just network packet loss
                stream.metrics.increment_packets_lost();
            }
        }

        let (packet, stream_pts) = queue_item.as_ref()
            .map(|item| (Some(&item.audio), Some(item.pts)))
            .unwrap_or_default();

        // pass packet through decode pipeline
        let mut buffer = [F::Frame::zeroed(); FRAMES_PER_PACKET * 2];
        let frames = stream.pipeline.process(packet, &mut buffer);
        let buffer = &buffer[0..frames];

        // increment frames decoded metric
        stream.metrics.increment_frames_decoded(FrameCount(frames));

        // lock output
        let Some(output) = stream.output.lock() else {
            // output has been stolen from us, exit thread
            break;
        };

        // get current output delay
        let delay = output.delay().unwrap();
        stats.output_latency = delay;
        stream.metrics.observe_buffer_delay(delay);

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

            let audio_offset = timing.real.delta(timing.play);
            stats.audio_latency = audio_offset;
            stream.metrics.observe_audio_offset(Some(audio_offset));
        } else {
            if stream.queue.is_empty() {
                stream.metrics.observe_audio_offset(None);
            }
        }

        // update stats
        *stats_tx.lock().unwrap() = stats.clone();

        // increment frames output metric
        stream.metrics.increment_frames_played(FrameCount(buffer.len()));

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
