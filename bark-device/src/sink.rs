use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::sync::{Arc, mpsc};

use bark_core::decode::AudioSink;
use bark_protocol::time::{SampleDuration, Timestamp};
use bark_protocol::types::AudioFrameF32;
use cpal::OutputCallbackInfo;
use cpal::traits::StreamTrait;
use cpal::{Stream, traits::{HostTrait, DeviceTrait}};
use futures::ready;
use futures::future::Future;
use futures::task::AtomicWaker;
use ringbuf::SharedRb;

use crate::{config, OpenError};

type RingBuffer = SharedRb<AudioFrameF32, Vec<MaybeUninit<AudioFrameF32>>>;
type Producer = ringbuf::Producer<AudioFrameF32, Arc<RingBuffer>>;
type Consumer = ringbuf::Consumer<AudioFrameF32, Arc<RingBuffer>>;

pub struct Sink {
    buffer: Producer,
    shared: Arc<Shared>,
    // must be held alive for the stream to keep running
    // stream ends on drop:
    _handle: StreamHandle,
}

struct Shared {
    notify: AtomicWaker,
    /// output latency of underlying device (not including buffer latency)
    /// in microseconds
    latency: AtomicLatency,
}

pub fn open(buffer_latency: SampleDuration) -> Result<Sink, OpenError> {
    let ringbuf = RingBuffer::new(buffer_latency.as_frame_buffer_offset());
    let (producer, consumer) = ringbuf.split();

    let shared = Arc::new(Shared {
        notify: AtomicWaker::new(),
        latency:  AtomicLatency::default(),
    });

    let handle = start_stream_thread(shared.clone(), consumer)?;

    Ok(Sink {
        buffer: producer,
        shared,
        _handle: handle,
    })
}

struct StreamHandle {
    // we use this channel as a drop guard to indicate to the device
    // thread that it should stop the stream and terminate:
    _guard: mpsc::SyncSender<()>,
}

// This function exists because `cpal::Stream` is not Send across all
// platforms, so we need to start the stream in the same thread we hold +
// drop it on.
fn start_stream_thread(shared: Arc<Shared>, consumer: Consumer) -> Result<StreamHandle, OpenError> {
    let (result_tx, result_rx) = mpsc::sync_channel(0);
    let (guard_tx, guard_rx) = mpsc::sync_channel(0);

    bark_util::thread::start("bark/device", move || {
        match start_stream(shared, consumer) {
            Err(error) => {
                let _  = result_tx.send(Err(error));
            }
            Ok(stream) => {
                let _ = result_tx.send(Ok(()));
                // receive on guard_rx to hold stream alive as long as
                // StreamHandle is held alive, then terminate.
                let _ = guard_rx.recv();
                drop(stream);
            }
        }
    });

    match result_rx.recv() {
        Ok(Ok(())) => Ok(StreamHandle { _guard: guard_tx }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(OpenError::ThreadError),
    }
}

fn start_stream(shared: Arc<Shared>, consumer: Consumer) -> Result<Stream, OpenError> {
    let host = cpal::default_host();

    let device = host.default_input_device()
        .ok_or(OpenError::NoDeviceAvailable)?;

    let config = config::for_device(&device)?;

    let stream = device.build_output_stream(
        &config,
        {
            let shared = shared.clone();
            let mut consumer = consumer;
            let mut initialized_thread = false;

            move |data: &mut [f32], info: &OutputCallbackInfo| {
                // on first call, try to set thread name + realtime prio:
                if !initialized_thread {
                    bark_util::thread::set_name("bark/audio");
                    bark_util::thread::set_realtime_priority();
                    initialized_thread = true;
                }

                // calculate output latency
                let ts = info.timestamp();
                let latency = ts.playback.duration_since(&ts.callback);
                let latency = latency.unwrap_or_default();
                let latency = SampleDuration::from_std_duration_lossy(latency);
                shared.latency.store(latency);

                // assert data only contains complete frames:
                assert!(data.len() % usize::from(bark_protocol::CHANNELS) == 0);
                let data = AudioFrameF32::from_interleaved_slice_mut(data);

                // read requested samples from ringbuffer:
                let n = consumer.pop_slice(data);

                // check for underrun and zero any remaining output buffer:
                if n < data.len() {
                    data[n..].fill(AudioFrameF32::zero());
                    // TODO signal underrun
                }

                // wake producer thread:
                shared.notify.wake();
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

    Ok(stream)
}

impl Sink {
    /// `data` must contain complete frames
    pub fn poll_write(&mut self, cx: &Context, data: &[AudioFrameF32]) -> Poll<(Timestamp, usize)> {
        // take current amount of data in buffer and current device latency
        // to estimate when the audio data written in this call will be played
        // by the device.
        let now = Timestamp::from_micros_lossy(bark_util::time::now());
        let buffered = self.buffer.capacity() - self.buffer.free_len();
        let buffer_latency = SampleDuration::from_frame_count(u64::try_from(buffered).unwrap());
        let device_latency = self.shared.latency.load();
        let latency = buffer_latency + device_latency;
        log::trace!("latency={}usec", latency.to_std_duration_lossy().as_micros());
        let pts = now + latency;

        let n = self.buffer.push_slice(data);

        if n == 0 && data.len() > 0 {
            self.shared.notify.register(cx.waker());
            log::trace!("pending");
            Poll::Pending
        } else {
            log::trace!("wrote {n}");
            Poll::Ready((pts, n))
        }
    }

    // async fn write_partial(&mut self, data: &[AudioFrameF32]) -> (Timestamp, usize) {
    //     poll_fn(|cx| self.poll_write(cx, data)).await
    // }

    // pub async fn write_all(&mut self, mut data: &[AudioFrameF32]) -> Timestamp {
    //     // do first write, taking pts (since the timestamp is the time the
    //     // _first_ sample in the buffer is played by the device)
    //     let (pts, n) = self.write_partial(data).await;
    //     data = &data[n..];

    //     while data.len() > 0 {
    //         let (_, n) = self.write_partial(data).await;
    //         data = &data[n..];
    //     }

    //     pts
    // }
}

impl AudioSink for Sink {
    type WriteFuture<'a> = WriteAudio<'a>;
    fn write<'a>(&'a mut self, audio: &'a [AudioFrameF32]) -> Self::WriteFuture<'a> {
        WriteAudio::new(self, audio)
    }
}

pub struct WriteAudio<'a> {
    sink: &'a mut Sink,
    data: &'a [AudioFrameF32],
    pts: Option<Timestamp>,
}

impl<'a> WriteAudio<'a> {
    pub fn new(sink: &'a mut Sink, data: &'a [AudioFrameF32]) -> Self {
        WriteAudio { sink, data, pts: None }
    }
}

impl<'a> Future for WriteAudio<'a> {
    type Output = Timestamp;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Timestamp> {
        let this = Pin::into_inner(self);

        loop {
            let (pts, n) = ready!(this.sink.poll_write(cx, this.data));
            let pts = *this.pts.get_or_insert(pts);
            this.data = &this.data[n..];

            if this.data.len() == 0 {
                return Poll::Ready(pts);
            }
        }
    }
}

#[derive(Default)]
struct AtomicLatency(AtomicUsize);

impl AtomicLatency {
    pub fn store(&self, latency: SampleDuration) {
        let frames = usize::try_from(latency.to_frame_count()).unwrap();
        self.0.store(frames, Ordering::Relaxed)
    }

    pub fn load(&self) -> SampleDuration {
        let frames = self.0.load(Ordering::Relaxed);
        SampleDuration::from_frame_count(u64::try_from(frames).unwrap())
    }
}
