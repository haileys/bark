use cpal::{StreamConfig, BufferSize, SupportedBufferSize, SampleFormat};
use cpal::traits::DeviceTrait;
use derive_more::From;

pub const SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

#[derive(Debug, From)]
pub enum ConfigError {
    EnumerateStreamConfigs(cpal::SupportedStreamConfigsError),
    NoSupportedStreamConfig,
}

pub fn config_for_device(device: &cpal::Device) -> Result<StreamConfig, ConfigError> {
    let configs = device.supported_input_configs()?;

    let config = configs
        .filter(|config| config.sample_format() == SAMPLE_FORMAT)
        .filter(|config| config.channels() == bark_protocol::CHANNELS.0)
        .nth(0)
        .ok_or(ConfigError::NoSupportedStreamConfig)?;

    let buffer_size = match config.buffer_size() {
        SupportedBufferSize::Range { min, .. } => {
            std::cmp::max(*min, bark_protocol::FRAMES_PER_PACKET as u32)
        }
        SupportedBufferSize::Unknown => {
            bark_protocol::FRAMES_PER_PACKET as u32
        }
    };

    Ok(StreamConfig {
        channels: bark_protocol::CHANNELS.0,
        sample_rate: cpal::SampleRate(bark_protocol::SAMPLE_RATE.0),
        buffer_size: BufferSize::Fixed(buffer_size),
    })
}
