use alsa::{Direction, pcm::IoFormat};
use bark_core::audio::{self, SampleFormat};
use bark_protocol::time::SampleDuration;
use nix::errno::Errno;
use thiserror::Error;

use crate::audio::config::{PCM, DeviceOpt, OpenError};

pub struct Output<S> {
    pcm: PCM<S>,
}

#[derive(Debug, Error)]
pub enum WriteAudioError {
    #[error("alsa: {0}")]
    Alsa(#[from] alsa::Error),
}

impl<S: SampleFormat + IoFormat> Output<S> {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        let pcm = PCM::open(&opt, Direction::Playback)?;
        Ok(Output { pcm })
    }

    pub fn write(&self, mut audio: &[S::Frame]) -> Result<(), WriteAudioError> {
        while audio.len() > 0 {
            let n = self.write_partial(audio)?;
            audio = &audio[n..];
        }

        Ok(())
    }

    fn write_partial(&self, audio: &[S::Frame]) -> Result<usize, WriteAudioError> {
        let io = self.pcm.io();

        loop {
            // try to write audio
            let err = match io.writei(audio::as_interleaved(audio)) {
                Ok(n) => { return Ok(n) },
                Err(e) => e,
            };

            // handle recoverable errors
            match err.errno() {
                | Errno::EPIPE // underrun
                | Errno::ESTRPIPE // stream suspended
                | Errno::EINTR // interrupted syscall
                => {
                    log::warn!("recovering from error: {}", err.errno());
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
