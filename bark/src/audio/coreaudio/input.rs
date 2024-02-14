use bark_core::audio::Frame;
use bark_protocol::time::Timestamp;

use crate::audio::config::DeviceOpt;
use crate::audio::coreaudio::Disconnected;
use crate::audio::OpenError;

pub struct Input;

impl Input {
    pub fn new(_: DeviceOpt) -> Result<Self, OpenError> {
        unimplemented!("can't stream from macOS");
    }

    pub fn read(&self, _: &mut [Frame]) -> Result<Timestamp, Disconnected> {
        unimplemented!("can't stream from macOS");
    }
}
