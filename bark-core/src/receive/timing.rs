use core::time::Duration;

use bark_protocol::time::{Timestamp, SampleDuration};
use bark_protocol::SampleRate;

pub struct RateAdjust {
    slew: bool,
}

#[derive(Copy, Clone)]
pub struct Timing {
    pub real: Timestamp,
    pub play: Timestamp,
}

impl RateAdjust {
    pub fn new() -> Self {
        RateAdjust {
            slew: false
        }
    }

    pub fn slew(&self) -> bool {
        self.slew
    }

    pub fn sample_rate(&mut self, timing: Timing) -> SampleRate {
        self.adjusted_rate(timing).unwrap_or(bark_protocol::SAMPLE_RATE)
    }

    fn adjusted_rate(&mut self, timing: Timing) -> Option<SampleRate> {
        // parameters, maybe these could be cli args?
        let start_slew_threshold = Duration::from_micros(2000);
        let stop_slew_threshold = Duration::from_micros(1000);
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

        // clamp any potential slow down to 1%, we shouldn't ever get too far
        // ahead of the stream
        let rate = std::cmp::max(base_sample_rate * 99 / 100, rate);

        // let the speed up run much higher, but keep it reasonable still
        let rate = std::cmp::min(base_sample_rate * 101 / 100, rate);

        self.slew = true;
        Some(SampleRate(u32::try_from(rate).unwrap()))
    }
}
