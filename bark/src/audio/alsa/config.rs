use alsa::{Direction, PCM, pcm::{HwParams, Format, Access}, ValueOr};
use bark_protocol::time::SampleDuration;
use thiserror::Error;

use crate::audio::config::DeviceOpt;

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("alsa error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("invalid period size (min = {min}, max = {max})")]
    InvalidPeriodSize { min: i64, max: i64 },
    #[error("invalid buffer size (min = {min}, max = {max})")]
    InvalidBufferSize { min: i64, max: i64 },
}

pub fn open_pcm(opt: &DeviceOpt, direction: Direction)
    -> Result<PCM, OpenError>
{
    let device_name = opt.device.as_deref().unwrap_or("default");
    let pcm = PCM::new(device_name, direction, false)?;

    {
        let hwp = HwParams::any(&pcm)?;
        hwp.set_channels(bark_protocol::CHANNELS.0.into())?;
        hwp.set_rate(bark_protocol::SAMPLE_RATE.0, ValueOr::Nearest)?;
        hwp.set_format(Format::float())?;
        hwp.set_access(Access::RWInterleaved)?;
        set_period_size(&hwp, opt.period)?;
        set_buffer_size(&hwp, opt.buffer)?;
        pcm.hw_params(&hwp)?;
    }

    {
        let hwp = pcm.hw_params_current()?;
        let swp = pcm.sw_params_current()?;
        swp.set_start_threshold(hwp.get_buffer_size()?)?;
    }

    let (buffer, period) = pcm.get_params()?;
    log::info!("opened ALSA with buffer_size={buffer}, period_size={period}");

    Ok(pcm)
}

// period is the size of the discrete chunks of data that are sent to hardware
fn set_period_size(hwp: &HwParams, period: SampleDuration)
    -> Result<(), OpenError>
{
    let min = hwp.get_period_size_min()?;
    let max = hwp.get_period_size_max()?;

    let period = period.to_frame_count().try_into().ok()
        .filter(|size| { *size >= min && *size <= max })
        .ok_or(OpenError::InvalidPeriodSize { min, max })?;

    hwp.set_period_size(period, ValueOr::Nearest)?;

    Ok(())
}

// period is the size of the discrete chunks of data that are sent to hardware
fn set_buffer_size(hwp: &HwParams, buffer: SampleDuration)
    -> Result<(), OpenError>
{
    let min = hwp.get_buffer_size_min()?;
    let max = hwp.get_buffer_size_max()?;

    let buffer = buffer.to_frame_count().try_into().ok()
        .filter(|size| *size >= min && *size <= max)
        .ok_or(OpenError::InvalidBufferSize { min, max })?;

    hwp.set_buffer_size(buffer)?;

    Ok(())
}
