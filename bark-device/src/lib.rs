pub mod config;
pub mod env;
pub mod sink;
pub mod source;

#[derive(Debug, derive_more::From)]
pub enum OpenError {
    NoDeviceAvailable,
    Configure(config::ConfigError),
    BuildStream(cpal::BuildStreamError),
    StartStream(cpal::PlayStreamError),
    ThreadError,
}
