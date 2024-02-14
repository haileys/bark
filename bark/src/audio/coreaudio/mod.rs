use thiserror::Error;

pub mod output;
pub mod input;

#[derive(Debug, Error)]
#[error("audio disconnected")]
pub struct Disconnected;
