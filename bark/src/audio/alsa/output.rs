use alsa::Direction;
use alsa::pcm::PCM;
use bark_core::audio::{Frame, self};
use bark_protocol::time::SampleDuration;

use crate::audio::config::DeviceOpt;
use crate::audio::alsa::config::{self, OpenError};

pub struct Output {
    pcm: PCM,
}

impl Output {
    pub fn new(opt: &DeviceOpt) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(opt, Direction::Playback)?;
        Ok(Output { pcm })
    }

    pub fn write(&self, mut audio: &[Frame]) -> Result<(), alsa::Error> {
        while audio.len() > 0 {
            let n = self.write_partial(audio)?;
            audio = &audio[n..];
        }

        Ok(())
    }

    fn write_partial(&self, audio: &[Frame]) -> Result<usize, alsa::Error> {
        let io = unsafe {
            // the checked versions of this function call
            // snd_pcm_hw_params_current which mallocs under the hood
            self.pcm.io_unchecked::<f32>()
        };

        loop {
            // try to write audio
            let err = match io.writei(audio::as_interleaved(audio)) {
                Ok(n) => { return Ok(n) },
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

    pub fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = self.pcm.delay()?;
        Ok(SampleDuration::from_frame_count(frames.try_into().unwrap()))
    }
}
