use core::time::Duration;

use bark_protocol::time::{Timestamp, SampleDuration};
use bark_protocol::SampleRate;

pub struct RateController {
    slew: bool,
    rate: SampleRate,
}

#[derive(Copy, Clone)]
pub struct Timing {
    pub real: Timestamp,
    pub play: Timestamp,
}

impl RateController {
    pub fn new() -> Self {
        RateController {
            slew: false,
            rate: bark_protocol::SAMPLE_RATE,
        }
    }

    pub fn slew(&self) -> bool {
        self.slew
    }

    pub fn rate(&self) -> SampleRate {
        self.rate
    }

    pub fn measure(&mut self, timing: Timing) {
        self.rate = self.adjusted_rate(timing).unwrap_or(bark_protocol::SAMPLE_RATE);
    }

    fn adjusted_rate(&mut self, timing: Timing) -> Option<SampleRate> {
        // parameters, maybe these could be cli args?
        let start_slew_threshold = Duration::from_micros(500);
        let stop_slew_threshold = Duration::from_micros(100);

        // turn them into native units
        let start_slew_threshold = SampleDuration::from_std_duration_lossy(start_slew_threshold);
        let stop_slew_threshold = SampleDuration::from_std_duration_lossy(stop_slew_threshold);

        let offset = timing.real.delta(timing.play);

        if offset.abs() < stop_slew_threshold {
            self.slew = false;
            return None;
        }

        if offset.abs() < start_slew_threshold && !self.slew {
            return None;
        }

        let base_sample_rate = i64::from(bark_protocol::SAMPLE_RATE);

        let rate_adjust = offset.as_frames().pow(3) / 48;
        let rate = base_sample_rate + rate_adjust;

        // clamp any potential rate adjustment to 1%, we shouldn't ever get too far
        // ahead of the stream
        let rate = std::cmp::max(base_sample_rate * 99 / 100, rate);
        let rate = std::cmp::min(base_sample_rate * 101 / 100, rate);

        self.slew = true;
        Some(SampleRate(u32::try_from(rate).unwrap()))
    }
}
