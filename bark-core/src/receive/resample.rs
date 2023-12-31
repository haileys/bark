use soxr::Soxr;
use soxr::format::Stereo;

use crate::audio::{FrameCount, SampleFormat};

pub struct Resampler<S: SampleFormat> {
    soxr: Soxr<Stereo<S>>,
}

pub struct ProcessResult {
    pub input_read: FrameCount,
    pub output_written: FrameCount,
}

impl<S: SampleFormat> Resampler<S> {
    pub fn new() -> Self {
        let rate = bark_protocol::SAMPLE_RATE.0 as f64;
        let soxr = Soxr::variable_rate(rate, rate).unwrap();
        Resampler { soxr }
    }

    pub fn set_input_rate(&mut self, rate: u32) -> Result<(), soxr::Error> {
        let input = rate as f64;
        let output = bark_protocol::SAMPLE_RATE.0 as f64;
        self.soxr.set_rates(input, output, 0)
    }

    pub fn process(&mut self, input: &[S::Frame], output: &mut [S::Frame])
        -> Result<ProcessResult, soxr::Error>
    {
        let input: &[[S; 2]] = bytemuck::must_cast_slice(input);
        let output: &mut [[S; 2]] = bytemuck::must_cast_slice_mut(output);

        let result = self.soxr.process(input, output)?;

        Ok(ProcessResult {
            input_read: FrameCount(result.input_frames),
            output_written: FrameCount(result.output_frames),
        })
    }
}
