use alsa::Direction;
use alsa::pcm::PCM;
use bark_protocol::CHANNELS;
use bark_protocol::time::SampleDuration;
use nix::errno::Errno;
use thiserror::Error;

use crate::audio::config::{self, DeviceOpt, OpenError};

pub struct Output {
    pcm: PCM,
}

#[derive(Debug, Error)]
pub enum WriteAudioError {
    #[error("alsa: {0}")]
    Alsa(#[from] alsa::Error),
}

impl Output {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        let pcm = config::open_pcm(&opt, Direction::Playback)?;
        Ok(Output { pcm })
    }

    pub fn write(&self, mut audio: &[f32]) -> Result<(), WriteAudioError> {
        while audio.len() > 0 {
            let n = self.write_partial(audio)?;
            audio = &audio[n..];
        }

        Ok(())
    }

    fn write_partial(&self, audio: &[f32]) -> Result<usize, WriteAudioError> {
        let io = unsafe {
            // the checked versions of this function call
            // snd_pcm_hw_params_current which mallocs under the hood
            self.pcm.io_unchecked::<f32>()
        };

        loop {
            // try to write audio
            let err = match io.writei(audio) {
                Ok(n) => {
                    return Ok(n * CHANNELS.0 as usize);
                }
                Err(e) => e,
            };

            // handle recoverable errors
            match err.errno() {
                | Errno::EPIPE // underrun
                | Errno::ESTRPIPE // stream suspended
                | Errno::EINTR // interrupted syscall
                => {
                    eprintln!("recovering from error: {}", err.errno());
                    // try to recover
                    self.pcm.recover(err.errno() as i32, false)?;
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
