use bark_core::audio::Frame;
use bark_protocol::time::{SampleDuration, Timestamp};
use thiserror::Error;

use self::config::DeviceOpt;

#[cfg(target_os = "linux")]
pub mod alsa;

pub mod config;

#[derive(Debug, Error)]
#[error(transparent)]
pub enum OpenError {
    #[cfg(target_os = "linux")]
    Alsa(#[from] alsa::config::OpenError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub enum Error {
    #[cfg(target_os = "linux")]
    Alsa(#[from] ::alsa::Error),
}

pub struct Input {
    #[cfg(target_os = "linux")]
    alsa: alsa::input::Input,
}

#[cfg(target_os = "linux")]
impl Input {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        Ok(Input {
            alsa: alsa::input::Input::new(opt)?,
        })
    }

    pub fn read(&self, audio: &mut [Frame]) -> Result<Timestamp, Error> {
        Ok(self.alsa.read(audio)?)
    }
}

pub struct Output {
    #[cfg(target_os = "linux")]
    alsa: alsa::output::Output,
}

#[cfg(target_os = "linux")]
impl Output {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        Ok(Output {
            alsa: alsa::output::Output::new(opt)?,
        })
    }

    pub fn write(&self, audio: &[Frame]) -> Result<(), Error> {
        Ok(self.alsa.write(audio)?)
    }

    pub fn delay(&self) -> Result<SampleDuration, Error> {
        Ok(self.alsa.delay()?)
    }
}
