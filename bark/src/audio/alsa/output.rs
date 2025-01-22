use std::marker::PhantomData;

use alsa::Direction;
use alsa::pcm::{IoFormat, PCM};

use bark_core::audio::{self, Format, Frames, F32, S16};
use bark_protocol::time::SampleDuration;

use crate::audio::config::DeviceOpt;
use crate::audio::alsa::config::{self, OpenError};
use crate::stats::ReceiverMetrics;

pub struct Output<F: Format> {
    inner: Inner,
    _phantom: PhantomData<F>,
}

struct Inner {
    pcm: PCM,
    metrics: ReceiverMetrics,
}

impl<F: Format> Output<F> {
    pub fn new(opt: &DeviceOpt, metrics: ReceiverMetrics) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(opt, F::KIND, Direction::Playback)?;

        Ok(Output {
            inner: Inner {
                pcm,
                metrics,
            },
            _phantom: PhantomData,
        })
    }

    pub fn write(&self, frames: &[F::Frame]) -> Result<(), alsa::Error> {
        match F::frames(frames) {
            Frames::S16(frames) => write_impl::<S16>(&self.inner, frames),
            Frames::F32(frames) => write_impl::<F32>(&self.inner, frames),
        }
    }

    pub fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = recover(&self.inner, || self.inner.pcm.delay())?;
        let frames = u64::try_from(frames).expect("pcm delay is negative");
        Ok(SampleDuration::from_frame_count_u64(frames))
    }
}

fn recover<T>(output: &Inner, func: impl Fn() -> Result<T, alsa::Error>) -> Result<T, alsa::Error> {
    loop {
        let err = match func() {
            Ok(value) => { return Ok(value); }
            Err(err) => err,
        };

        // handle recoverable errors
        match err.errno() {
            | libc::EPIPE // underrun
            | libc::ESTRPIPE // stream suspended
            | libc::EINTR // interrupted syscall
            => {
                // try to recover
                output.pcm.recover(err.errno(), false)?;

                if err.errno() == libc::EPIPE {
                    output.metrics.buffer_underruns.increment();
                }
            }
            _ => { return Err(err); }
        }
    }
}

fn write_impl<F: Format>(output: &Inner, mut frames: &[F::Frame])
    -> Result<(), alsa::Error>
    where F::Sample: IoFormat
{
    while frames.len() > 0 {
        let n = write_partial_impl::<F>(output, frames)?;
        frames = &frames[n..];
    }

    Ok(())
}

fn write_partial_impl<F: Format>(output: &Inner, samples: &[F::Frame])
    -> Result<usize, alsa::Error>
    where F::Sample: IoFormat
{
    let io = unsafe {
        // the checked versions of this function call
        // snd_pcm_hw_params_current which mallocs under the hood
        output.pcm.io_unchecked::<F::Sample>()
    };

    recover(output, || io.writei(audio::as_interleaved::<F>(samples)))
}
