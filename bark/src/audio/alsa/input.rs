use std::marker::PhantomData;

use alsa::Direction;
use alsa::pcm::{IoFormat, PCM};
use bark_core::audio::{self, Format, FramesMut, F32, S16};
use bark_protocol::time::{Timestamp, SampleDuration};

use crate::audio::config::DeviceOpt;
use crate::audio::alsa::config::{self, OpenError};
use crate::time;

pub struct Input<F: Format> {
    pcm: PCM,
    _phantom: PhantomData<F>,
}

impl<F: Format> Input<F> {
    pub fn new(opt: &DeviceOpt) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(opt, F::KIND, Direction::Capture)?;
        Ok(Input { pcm, _phantom: PhantomData })
    }

    pub fn read(&self, frames: &mut [F::Frame]) -> Result<Timestamp, alsa::Error> {
        let now = Timestamp::from_micros_lossy(time::now());
        let timestamp = now.saturating_sub(self.delay()?);

        match F::frames_mut(frames) {
            FramesMut::S16(frames) => read_impl::<S16>(&self.pcm, frames)?,
            FramesMut::F32(frames) => read_impl::<F32>(&self.pcm, frames)?,
        }

        Ok(timestamp)
    }

    fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = self.pcm.delay()?;
        Ok(SampleDuration::from_frame_count(frames.try_into().unwrap()))
    }
}

fn read_impl<F: Format>(pcm: &PCM, mut frames: &mut [F::Frame])
    -> Result<(), alsa::Error>
    where F::Sample: IoFormat
{
    while frames.len() > 0 {
        let n = read_partial_impl::<F>(pcm, frames)?;
        frames = &mut frames[n..];
    }

    Ok(())
}

fn read_partial_impl<F: Format>(pcm: &PCM, frames: &mut [F::Frame])
    -> Result<usize, alsa::Error>
    where F::Sample: IoFormat
{
    let io = unsafe {
        // the checked versions of this function call
        // snd_pcm_hw_params_current which mallocs under the hood
        pcm.io_unchecked::<F::Sample>()
    };

    loop {
        // try to write audio
        let err = match io.readi(audio::as_interleaved_mut::<F>(frames)) {
            Ok(n) => { return Ok(n) }
            Err(e) => e,
        };

        // handle recoverable errors
        match err.errno() {
            | libc::EPIPE // underrun
            | libc::ESTRPIPE // stream suspended
            | libc::EINTR // interrupted syscall
            => {
                log::warn!("recovering from error: {}", err.errno());
                // try to recover
                pcm.recover(err.errno(), false)?;
            }
            _ => { return Err(err.into()); }
        }
    }
}
