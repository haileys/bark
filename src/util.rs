use cpal::{StreamConfig, BufferSize, SupportedBufferSize};
use cpal::traits::DeviceTrait;

use crate::RunError;
use crate::protocol;

pub fn config_for_device(device: &cpal::Device) -> Result<StreamConfig, RunError> {
    let configs = device.supported_input_configs()
        .map_err(RunError::StreamConfigs)?;

    let config = configs
        .filter(|config| config.sample_format() == protocol::SAMPLE_FORMAT)
        .filter(|config| config.channels() == protocol::CHANNELS)
        .nth(0)
        .ok_or(RunError::NoSupportedStreamConfig)?;

    let buffer_size = match config.buffer_size() {
        SupportedBufferSize::Range { min, .. } => {
            std::cmp::max(*min, protocol::FRAMES_PER_PACKET as u32)
        }
        SupportedBufferSize::Unknown => {
            protocol::FRAMES_PER_PACKET as u32
        }
    };

    Ok(StreamConfig {
        channels: protocol::CHANNELS,
        sample_rate: protocol::SAMPLE_RATE,
        buffer_size: BufferSize::Fixed(buffer_size),
    })
}
