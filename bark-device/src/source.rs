use std::sync::{Mutex, Condvar, Arc};

use bark_protocol::types::TimestampMicros;
use cpal::{traits::{HostTrait, DeviceTrait, StreamTrait}, InputCallbackInfo, Stream};
use derive_more::From;
use heapless::Deque;

use crate::config::{self, ConfigError};

const QUEUE_CAPACITY: usize = 4;

pub struct Source {
    queue: Arc<Queue>,
    // must be held alive for the stream to keep running
    // stream ends on drop:
    _stream: Stream,
}

pub struct AudioPacket {
    pub timestamp: TimestampMicros,
    pub data: Vec<f32>,
}

#[derive(Debug, From)]
pub enum OpenError {
    NoDeviceAvailable,
    Configure(ConfigError),
    BuildStream(cpal::BuildStreamError),
    StartStream(cpal::PlayStreamError),
}

pub fn open() -> Result<Source, OpenError> {
    let host = cpal::default_host();

    let device = host.default_input_device()
        .ok_or(OpenError::NoDeviceAvailable)?;

    let config = config::for_device(&device)?;

    let queue = Arc::new(Queue::new());

    let stream = device.build_input_stream(
        &config,
        {
            let queue = queue.clone();
            let mut initialized_thread = false;

            move |data: &[f32], info: &InputCallbackInfo| {
                // take current time immediately:
                let timestamp = bark_util::time::now();

                // on first call, try to set thread name + realtime prio:
                if !initialized_thread {
                    bark_util::thread::set_name("bark/audio");
                    bark_util::thread::set_realtime_priority();
                    initialized_thread = true;
                }

                // assert data only contains complete frames:
                assert!(data.len() % usize::from(bark_protocol::CHANNELS) == 0);

                // calculate latency from capture to callback:
                let callback_ts = info.timestamp().callback;
                let capture_ts = info.timestamp().capture;
                let callback_latency = callback_ts.duration_since(&capture_ts).unwrap_or_default();

                // subtract latency from timestamp to get capture timestamp:
                let callback_latency_micros = u64::try_from(callback_latency.as_micros())
                    .expect("callback_latency: narrow u128 -> u64");

                let timestamp = TimestampMicros(timestamp.0 - callback_latency_micros);

                // force push packet to queue, overwriting any previous slots
                // if the receiver is running slow:
                queue.force_push(AudioPacket {
                    timestamp,
                    data: data.to_vec(),
                });
            }
        },
        {
            move |err| {
                log::error!("stream error: {err:?}");
            }
        },
        None,
    )?;

    stream.play()?;

    Ok(Source { queue, _stream: stream })
}

struct Queue {
    deque: Mutex<Deque<AudioPacket, QUEUE_CAPACITY>>,
    cond: Condvar,
}

impl Queue {
    pub fn new() -> Self {
        Queue {
            deque: Mutex::new(Deque::new()),
            cond: Condvar::new(),
        }
    }

    /// Push audio packet to queue, overwriting oldest item if full
    pub fn force_push(&self, packet: AudioPacket) {
        let mut deque = self.deque.lock().unwrap();

        // ensure there is always room:
        if deque.is_full() {
            deque.pop_front();
        }

        // push to back
        if let Err(_) = deque.push_back(packet) {
            unreachable!();
        }

        self.cond.notify_all();
    }
}

impl Source {
    /// Blocking wait for audio packet. Returns None if stream ended.
    pub fn read(&mut self) -> Option<AudioPacket> {
        let mut deque = self.queue.deque.lock().unwrap();

        loop {
            // if there's a packet at the front of the queue, return it:
            if let Some(packet) = deque.pop_front() {
                return Some(packet);
            }

            // if we are the only reference to the queue left, the other end
            // has hung up, and no more packets will ever be pushed:
            if Arc::strong_count(&self.queue) == 1 {
                return None;
            }

            // otherwise, wait on the cond var and try again:
            deque = self.queue.cond.wait(deque).unwrap();
        }
    }
}
