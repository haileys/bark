use bark_core::audio::Frame;
use bark_protocol::time::{SampleDuration, Timestamp};
use thiserror::Error;

use config::DeviceOpt;

#[cfg(target_os = "linux")]
pub mod alsa;

#[cfg(target_os = "macos")]
pub mod coreaudio;

pub mod config;

#[derive(Debug, Error)]
#[error(transparent)]
pub enum OpenError {
    #[cfg(target_os = "linux")]
    Alsa(#[from] alsa::config::OpenError),
    #[cfg(target_os = "macos")]
    CoreAudio(#[from] ::coreaudio::Error),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub enum Error {
    #[cfg(target_os = "linux")]
    Alsa(#[from] ::alsa::Error),
    #[cfg(target_os = "macos")]
    CoreAudio(#[from] coreaudio::Disconnected),
}

pub struct Input {
    #[cfg(target_os = "linux")]
    alsa: alsa::input::Input,
    #[cfg(target_os = "macos")]
    coreaudio: coreaudio::input::Input,
}

pub struct Output {
    #[cfg(target_os = "linux")]
    alsa: alsa::output::Output,
    #[cfg(target_os = "macos")]
    coreaudio: coreaudio::output::Output,
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

#[cfg(target_os = "macos")]
impl Input {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        Ok(Input {
            coreaudio: coreaudio::input::Input::new(opt)?,
        })
    }

    pub fn read(&self, audio: &mut [Frame]) -> Result<Timestamp, coreaudio::Disconnected> {
        Ok(self.coreaudio.read(audio)?)
    }
}

#[cfg(target_os = "linux")]
impl Output {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        Ok(Output {
            alsa: alsa::output::Output::new(opt)?,
        })
    }

    pub fn write(&mut self, audio: &[Frame]) -> Result<(), Error> {
        Ok(self.alsa.write(audio)?)
    }

    pub fn delay(&self) -> Result<SampleDuration, Error> {
        Ok(self.alsa.delay()?)
    }
}

#[cfg(target_os = "macos")]
impl Output {
    pub fn new(opt: DeviceOpt) -> Result<Self, OpenError> {
        Ok(Output {
            coreaudio: coreaudio::output::Output::new(opt)?,
        })
    }

    pub fn write(&mut self, audio: &[Frame]) -> Result<(), Error> {
        Ok(self.coreaudio.write(audio)?)
    }

    pub fn delay(&self) -> Result<SampleDuration, Error> {
        Ok(self.coreaudio.delay()?)
    }
}
