use alsa::Direction;
use alsa::pcm::PCM;
use bark_core::audio::{Frame, self};
use bark_protocol::time::{Timestamp, SampleDuration};

use crate::audio::config::DeviceOpt;
use crate::audio::alsa::config::{self, OpenError};
use crate::time;

pub struct Input {
    pcm: PCM,
}

impl Input {
    pub fn new(opt: &DeviceOpt) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(opt, Direction::Capture)?;
        Ok(Input { pcm })
    }

    pub fn read(&self, mut audio: &mut [Frame]) -> Result<Timestamp, alsa::Error> {
        let now = Timestamp::from_micros_lossy(time::now());
        let timestamp = now.saturating_sub(self.delay()?);

        while audio.len() > 0 {
            let n = self.read_partial(audio)?;
            audio = &mut audio[n..];
        }

        Ok(timestamp)
    }

    fn read_partial(&self, audio: &mut [Frame]) -> Result<usize, alsa::Error> {
        let io = unsafe {
            // the checked versions of this function call
            // snd_pcm_hw_params_current which mallocs under the hood
            self.pcm.io_unchecked::<f32>()
        };

        loop {
            // try to write audio
            let err = match io.readi(audio::as_interleaved_mut(audio)) {
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
                    self.pcm.recover(err.errno(), false)?;
                }
                _ => { return Err(err.into()); }
            }
        }
    }

    fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = self.pcm.delay()?;
        Ok(SampleDuration::from_frame_count(frames.try_into().unwrap()))
    }
}
