use bark_core::audio::Format;
use bark_protocol::time::{SampleDuration, Timestamp};
use thiserror::Error;

use crate::stats::server::ReceiverMetrics;

use self::config::DeviceOpt;

pub mod alsa;
pub mod config;

#[derive(Debug, Error)]
#[error(transparent)]
pub enum OpenError {
    Alsa(#[from] alsa::config::OpenError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub enum Error {
    Alsa(#[from] ::alsa::Error),
}

pub struct Input<F: Format> {
    alsa: alsa::input::Input<F>,
}

impl<F: Format> Input<F> {
    pub fn new(opt: &DeviceOpt) -> Result<Self, OpenError> {
        Ok(Input {
            alsa: alsa::input::Input::new(opt)?,
        })
    }

    pub fn read(&self, audio: &mut [F::Frame]) -> Result<Timestamp, Error> {
        Ok(self.alsa.read(audio)?)
    }
}

pub struct Output<F: Format> {
    alsa: alsa::output::Output<F>,
}

impl<F: Format> Output<F> {
    pub fn new(opt: &DeviceOpt, metrics: ReceiverMetrics) -> Result<Self, OpenError> {
        Ok(Output {
            alsa: alsa::output::Output::new(opt, metrics)?,
        })
    }

    pub fn write(&self, audio: &[F::Frame]) -> Result<(), Error> {
        Ok(self.alsa.write(audio)?)
    }

    pub fn delay(&self) -> Result<SampleDuration, Error> {
        Ok(self.alsa.delay()?)
    }
}
