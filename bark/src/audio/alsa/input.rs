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
    quantum: SampleDuration,
    _phantom: PhantomData<F>,
}

impl<F: Format> Input<F> {
    pub fn new(opt: &DeviceOpt) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(opt, F::KIND, Direction::Capture)?;
        let (_buffer, period) = pcm.get_params()?;
        Ok(Input {
            pcm,
            quantum: SampleDuration::from_frame_count_u64(period),
            _phantom: PhantomData,
        })
    }

    pub fn read(&self, frames: &mut [F::Frame]) -> Result<Timestamp, alsa::Error> {
        match F::frames_mut(frames) {
            FramesMut::S16(frames) => read_impl::<S16>(&self.pcm, frames)?,
            FramesMut::F32(frames) => read_impl::<F32>(&self.pcm, frames)?,
        }

        // calculate timestamp of this packet of audio.
        //
        // each quantum (aka period in ALSA terminology) of audio received
        // from ALSA is assumed to begin at the timestamp it first enters the
        // buffer.
        //
        // to calculate this time, take the current time, add the quantum, and
        // subtract the current buffer delay (number of frames currently in the
        // buffer + HW latency if applicable), making sure to compensate delay
        // for the number of frames we just read.
        //
        // when quantum > bark packet size, we'll make multiple successful
        // reads here without blocking, so the current time can be assumed to
        // be ~roughly the same for each packet in a quantum.

        let now = time::now();

        let delay = self.delay()?
            .add(SampleDuration::from_frame_count(frames.len()));

        let timestamp = Timestamp::from_micros_lossy(now)
            .add(self.quantum)
            .saturating_sub(delay);

        Ok(timestamp)
    }

    fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = self.pcm.delay()?;
        let frames = u64::try_from(frames).expect("pcm delay is negative");
        Ok(SampleDuration::from_frame_count_u64(frames))
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
