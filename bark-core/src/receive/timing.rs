use bark_protocol::time::{SampleDuration, Timestamp, TimestampDelta};
use bark_protocol::{SAMPLE_RATE, SampleRate};

const START_SLEW_THRESHOLD: SampleDuration = SampleDuration::from_frame_count(SAMPLE_RATE.0 as usize / 2);
const STOP_SLEW_THRESHOLD: SampleDuration = SampleDuration::from_frame_count(SAMPLE_RATE.0 as usize / 10);

const SAMPLE_RATE_F64: f64 = SAMPLE_RATE.0 as f64;
const K_P: f64 = 0.050;
const K_I: f64 = 0.005;
const K_D: f64 = 0.005;

pub struct RateController {
    integral: f64,
    prev_pts: Option<Timestamp>,
    prev_err: Option<f64>,
    // slew: bool,
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
            integral: 0.0,
            prev_pts: None,
            prev_err: None,
            rate: SAMPLE_RATE,
        }
    }

    pub fn slew(&self) -> bool {
        !self.sync()
    }

    pub fn sync(&self) -> bool {
        let Some(err) = self.prev_err else { return false };
        err < 0.001 && self.rate.0.abs_diff(SAMPLE_RATE.0) <= 1
    }

    pub fn rate(&self) -> SampleRate {
        self.rate
    }

    pub fn measure(&mut self, timing: Timing) {
        let error = timing.real.delta(timing.play);
        let e_t = (error.as_frames() as f64) / SAMPLE_RATE_F64;

        let prev_pts = self.prev_pts.replace(timing.play);
        let d_t = prev_pts.map(|prev|
            timing.play.delta(prev).as_frames() as f64 / SAMPLE_RATE_F64
        );

        let prev_err = self.prev_err.replace(e_t);
        let d_e = prev_err.map(|prev| e_t - prev);

        let p_term = K_P * e_t;

        let i_term = match d_t {
            Some(d_t) => {
                self.integral += e_t * d_t;
                K_I * self.integral
            }
            _ => 0.0
        };

        let d_term = match (d_e, d_t) {
            (Some(d_e), Some(d_t)) => {
                K_D * (d_e + d_t)
            }
            _ => 0.0,
        };

        let u_t = p_term + i_term + d_term;

        let rate_delta_f64 = f64::round(u_t * SAMPLE_RATE_F64);
        let rate_delta = rate_delta_f64 as i32;
        let rate = SAMPLE_RATE.0.strict_add_signed(rate_delta);
        self.rate = SampleRate(rate);
    }
}
