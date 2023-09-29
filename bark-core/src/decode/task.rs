use core::cmp;
use core::time::Duration;

use bark_protocol::{SampleRate, FRAMES_PER_PACKET};
use bark_protocol::time::{Timestamp, SampleDuration};
use bark_protocol::types::AudioFrameF32;
use bytemuck::Zeroable;
use derive_more::From;

use crate::consts::DECODE_BUFFER_FRAMES;

use super::{Receiver, AudioSink};
use super::resample::{Resampler, SpeexError};

pub struct Decode<R, S> {
    receiver: R,
    sink: S,
    adjust: RateAdjust,
    resampler: Resampler,
}

#[derive(Debug, From)]
pub enum NewDecodeError {
    AllocateResampler(SpeexError),
}

impl<R: Receiver, S: AudioSink> Decode<R, S> {
    pub fn new(receiver: R, sink: S) -> Result<Self, NewDecodeError> {
        Ok(Decode {
            receiver,
            sink,
            adjust: RateAdjust::new(),
            resampler: Resampler::new()?
        })
    }

    /// Run main decode loop. Cancellable.
    pub async fn run(mut self) -> ! {
        let mut buffer = [AudioFrameF32::zeroed(); DECODE_BUFFER_FRAMES];

        loop {
            // pull next segment from network task
            let segment = self.receiver.next_segment();

            // if segment is missing, write a packet's worth of silence to
            // the output and continue loop:
            let Some(segment) = segment else {
                let silence = &mut buffer[0..FRAMES_PER_PACKET];
                silence.fill(AudioFrameF32::zeroed());
                self.sink.write(silence).await;
                continue;
            };

            let mut input = segment.data.frames();
            let mut pts = segment.pts;

            while input.len() > 0 {
                match self.resampler.process_floats(input, &mut buffer) {
                    Ok(result) => {
                        // write resampled output:
                        let frames_written = result.output_written.to_frame_count();
                        let frames_written = usize::try_from(frames_written).unwrap();
                        let output = &buffer[0..frames_written];
                        let expected = self.sink.write(&output).await;

                        // send timing information to rate adjuster and
                        // update resampler sample rate:
                        let timing = Timing { play: pts, real: expected };
                        log::trace!("timing: stream_pts={pts:?}, real_pts={expected:?}");
                        let rate = self.adjust.sample_rate(timing);
                        if let Err(e) = self.resampler.set_input_rate(rate) {
                            log::error!("error adjusting resampler input rate: {e:?}");
                        }

                        // advance input:
                        let frames_read = result.input_read.to_frame_count();
                        let frames_read = usize::try_from(frames_read).unwrap();
                        input = &input[frames_read..];
                        pts += result.input_read;
                    }
                    Err(e) => {
                        log::error!("resampler error: {e:?}");
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Copy, Clone)]
struct Timing {
    pub real: Timestamp,
    pub play: Timestamp,
}

struct RateAdjust {
    slew: bool,
}

impl RateAdjust {
    pub fn new() -> Self {
        RateAdjust {
            slew: false
        }
    }

    pub fn sample_rate(&mut self, timing: Timing) -> SampleRate {
        self.adjusted_rate(timing).unwrap_or(bark_protocol::SAMPLE_RATE)
    }

    fn adjusted_rate(&mut self, timing: Timing) -> Option<SampleRate> {
        // parameters, maybe these could be cli args?
        let start_slew_threshold = Duration::from_micros(2000);
        let stop_slew_threshold = Duration::from_micros(100);
        let slew_target_duration = Duration::from_millis(500);

        // turn them into native units
        let start_slew_threshold = SampleDuration::from_std_duration_lossy(start_slew_threshold);
        let stop_slew_threshold = SampleDuration::from_std_duration_lossy(stop_slew_threshold);

        let frame_offset = timing.real.delta(timing.play);

        if frame_offset.abs() < stop_slew_threshold {
            self.slew = false;
            return None;
        }

        if frame_offset.abs() < start_slew_threshold && !self.slew {
            return None;
        }

        let slew_duration_duration = i64::try_from(slew_target_duration.as_micros()).unwrap();
        let base_sample_rate = i64::from(bark_protocol::SAMPLE_RATE);
        let rate_offset = frame_offset.as_frames() * 1_000_000 / slew_duration_duration;
        let rate = base_sample_rate + rate_offset;

        // clamp any potential slow down to 2%, we shouldn't ever get too far
        // ahead of the stream
        let rate = cmp::max(base_sample_rate * 98 / 100, rate);

        // let the speed up run much higher, but keep it reasonable still
        let rate = cmp::min(base_sample_rate * 2, rate);

        self.slew = true;
        Some(SampleRate(u32::try_from(rate).unwrap()))
    }
}
