use std::marker::PhantomData;

use alsa::Direction;
use alsa::pcm::{IoFormat, PCM};

use bark_core::audio::{self, Format, Frames, F32, S16};
use bark_protocol::time::SampleDuration;

use crate::audio::config::DeviceOpt;
use crate::audio::alsa::config::{self, OpenError};

pub struct Output<F: Format> {
    pcm: PCM,
    _phantom: PhantomData<F>,
}

impl<F: Format> Output<F> {
    pub fn new(opt: &DeviceOpt) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(opt, F::KIND, Direction::Playback)?;
        Ok(Output { pcm, _phantom: PhantomData })
    }

    pub fn write(&self, frames: &[F::Frame]) -> Result<(), alsa::Error> {
        match F::frames(frames) {
            Frames::S16(frames) => write_impl::<S16>(&self.pcm, frames),
            Frames::F32(frames) => write_impl::<F32>(&self.pcm, frames),
        }
    }

    pub fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = recover(&self.pcm, || self.pcm.delay())?;
        Ok(SampleDuration::from_frame_count(frames.try_into().unwrap()))
    }
}

fn recover<T>(pcm: &PCM, func: impl Fn() -> Result<T, alsa::Error>) -> Result<T, alsa::Error> {
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
                log::warn!("recovering from alsa error: {}", err.errno());
                // try to recover
                pcm.recover(err.errno(), false)?;
            }
            _ => { return Err(err); }
        }
    }
}

fn write_impl<F: Format>(pcm: &PCM, mut frames: &[F::Frame]) -> Result<(), alsa::Error>
    where F::Sample: IoFormat
{
    while frames.len() > 0 {
        let n = write_partial_impl::<F>(pcm, frames)?;
        frames = &frames[n..];
    }

    Ok(())
}

fn write_partial_impl<F: Format>(pcm: &PCM, samples: &[F::Frame]) -> Result<usize, alsa::Error>
    where F::Sample: IoFormat
{
    let io = unsafe {
        // the checked versions of this function call
        // snd_pcm_hw_params_current which mallocs under the hood
        pcm.io_unchecked::<F::Sample>()
    };

    recover(pcm, || io.writei(audio::as_interleaved::<F>(samples)))
}
