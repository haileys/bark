use alsa::{ValueOr, Direction};
use alsa::pcm::{PCM, HwParams, Format, Access};
use bark_protocol::time::SampleDuration;
use thiserror::Error;

pub struct Output {
    pcm: PCM,
}

#[derive(Debug, Error)]
pub enum OpenError {
    #[error("alsa: {0}")]
    Alsa(#[from] alsa::Error),
}

#[derive(Debug, Error)]
pub enum WriteAudioError {
    #[error("alsa: {0}")]
    Alsa(#[from] alsa::Error),
}

impl Output {
    pub fn new() -> Result<Self, OpenError> {
        let pcm = PCM::new("default", Direction::Playback, false)?;

        let period_size = bark_protocol::FRAMES_PER_PACKET;
        let buffer_size = period_size * 2;

        let hwp = HwParams::any(&pcm)?;
        hwp.set_channels(bark_protocol::CHANNELS.0.into())?;
        hwp.set_rate(bark_protocol::SAMPLE_RATE.0, ValueOr::Nearest)?;
        hwp.set_format(Format::float())?;
        hwp.set_access(Access::RWInterleaved)?;
        hwp.set_period_size(period_size.try_into().unwrap(), ValueOr::Nearest)?;
        hwp.set_buffer_size(buffer_size.try_into().unwrap())?;
        pcm.hw_params(&hwp)?;
        drop(hwp);

        let swp = pcm.sw_params_current()?;
        swp.set_start_threshold(buffer_size.try_into().unwrap())?;
        drop(swp);

        let (buffer, period) = pcm.get_params()?;
        eprintln!("opened ALSA with buffer_size={buffer}, period_size={period}");

        Ok(Output { pcm })
    }

    pub fn write(&self, audio: &[f32]) -> Result<(), WriteAudioError> {
        self.pcm.io_f32()?.writei(audio)?;
        Ok(())
    }

    pub fn delay(&self) -> Result<SampleDuration, alsa::Error> {
        let frames = self.pcm.delay()?;
        Ok(SampleDuration::from_frame_count(frames.try_into().unwrap()))
    }
}
