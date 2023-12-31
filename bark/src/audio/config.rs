use std::marker::PhantomData;

use alsa::{Direction, ValueOr};
use alsa::pcm::{HwParams, Format, Access, IoFormat};
use bark_core::audio::SampleFormat;
use bark_protocol::time::SampleDuration;
use thiserror::Error;

pub const DEFAULT_PERIOD: SampleDuration = SampleDuration::from_frame_count(120);
pub const DEFAULT_BUFFER: SampleDuration = SampleDuration::from_frame_count(360);

pub struct DeviceOpt {
    pub device: Option<String>,
    pub period: SampleDuration,
    pub buffer: SampleDuration,
}

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("alsa error: {0}")]
    Alsa(#[from] alsa::Error),
    #[error("invalid period size (min = {min}, max = {max})")]
    InvalidPeriodSize { min: i64, max: i64 },
    #[error("invalid buffer size (min = {min}, max = {max})")]
    InvalidBufferSize { min: i64, max: i64 },
}

pub struct PCM<S> {
    alsa: alsa::PCM,
    _phantom: PhantomData<S>,
}

impl<S: SampleFormat + IoFormat> PCM<S> {
    pub fn open(opt: &DeviceOpt, direction: Direction) -> Result<Self, OpenError> {
        let device_name = opt.device.as_deref().unwrap_or("default");
        let pcm = alsa::PCM::new(device_name, direction, false)?;

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

        Ok(PCM {
            alsa: pcm,
            _phantom: PhantomData,
        })
    }

    pub fn io(&self) -> alsa::pcm::IO<'_, S> {
        unsafe {
            // the checked versions of this function call
            // snd_pcm_hw_params_current which mallocs under the hood.
            // fortunately, we encode the IO format as a type parameter anyway,
            // so we can safely use the unchecked function
            self.alsa.io_unchecked::<S>()
        }
    }

    pub fn delay(&self) -> alsa::Result<i64> {
        self.alsa.delay()
    }

    pub fn recover(&self, err: i32, silent: bool) -> alsa::Result<()> {
        self.alsa.recover(err, silent)
    }
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
