use alsa::Direction;
use alsa::pcm::IoFormat;
use bark_core::audio::{self, SampleFormat};
use bark_protocol::time::{Timestamp, SampleDuration};
use nix::errno::Errno;
use thiserror::Error;

use crate::audio::config::{PCM, DeviceOpt, OpenError};
use crate::time;

pub struct Input<S> {
    pcm: PCM<S>,
}

#[derive(Debug, Error)]
pub enum ReadAudioError {
    #[error("alsa: {0}")]
    Alsa(#[from] alsa::Error),
}

impl<S: SampleFormat + IoFormat> Input<S> {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        let pcm = PCM::open(&opt, Direction::Capture)?;
        Ok(Input { pcm })
    }

    pub fn read(&self, mut audio: &mut [S::Frame]) -> Result<Timestamp, ReadAudioError> {
        let now = Timestamp::from_micros_lossy(time::now());
        let timestamp = now.saturating_sub(self.delay()?);

        while audio.len() > 0 {
            let n = self.read_partial(audio)?;
            audio = &mut audio[n..];
        }

        Ok(timestamp)
    }

    fn read_partial(&self, audio: &mut [S::Frame]) -> Result<usize, ReadAudioError> {
        let io = self.pcm.io();

        loop {
            // try to write audio
            let err = match io.readi(audio::as_interleaved_mut(audio)) {
                Ok(n) => { return Ok(n) }
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

    fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = self.pcm.delay()?;
        Ok(SampleDuration::from_frame_count(frames.try_into().unwrap()))
    }
}
