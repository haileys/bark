use crate::types::TimestampMicros;
use crate::{SAMPLE_RATE, FRAMES_PER_PACKET};

/// A timestamp with implicit denominator SAMPLE_RATE
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn to_micros_lossy(&self) -> TimestampMicros {
        let ts = u128::from(self.0);
        let micros = (ts * 1_000_000) / u128::from(SAMPLE_RATE.0);
        let micros = u64::try_from(micros)
            .expect("can't narrow timestamp to u64");
        TimestampMicros(micros)
    }

    pub fn from_micros_lossy(micros: TimestampMicros) -> Timestamp {
        let micros = u128::from(micros.0);
        let ts = (micros * u128::from(SAMPLE_RATE.0)) / 1_000_000;
        let ts = u64::try_from(ts)
            .expect("can't narrow timestamp to u64");
        Timestamp(ts)
    }

    pub fn add(&self, duration: SampleDuration) -> Timestamp {
        Timestamp(self.0.checked_add(duration.0).unwrap())
    }

    pub fn saturating_sub(&self, duration: SampleDuration) -> Timestamp {
        Timestamp(self.0.saturating_sub(duration.0))
    }

    pub fn saturating_duration_since(&self, other: Timestamp) -> SampleDuration {
        SampleDuration(self.0.saturating_sub(other.0))
    }

    pub fn duration_since(&self, other: Timestamp) -> SampleDuration {
        SampleDuration(self.0.checked_sub(other.0).unwrap())
    }

    pub fn delta(&self, other: Timestamp) -> TimestampDelta {
        let self_ = i64::try_from(self.0).expect("u64 -> i64 in Timestamp::delta");
        let other = i64::try_from(other.0).expect("u64 -> i64 in Timestamp::delta");
        TimestampDelta(self_.checked_sub(other).expect("underflow in Timestamp::delta"))
    }

    pub fn adjust(&self, delta: TimestampDelta) -> Timestamp {
        Timestamp(self.0.checked_add_signed(delta.0).unwrap())
    }
}

/// A duration with implicit denominator SAMPLE_RATE
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleDuration(u64);

impl SampleDuration {
    pub const ONE_PACKET: SampleDuration = SampleDuration::from_frame_count(FRAMES_PER_PACKET);

    pub const fn zero() -> Self {
        SampleDuration(0)
    }

    pub const fn from_frame_count_u64(frames: u64) -> Self {
        SampleDuration(frames)
    }

    #[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
    pub const fn from_frame_count(frames: usize) -> Self {
        SampleDuration(frames as u64)
    }

    pub fn to_frame_count(self) -> u64 {
        self.0
    }

    pub fn from_std_duration_lossy(duration: core::time::Duration) -> SampleDuration {
        let duration = (duration.as_micros() * u128::from(SAMPLE_RATE)) / 1_000_000;
        let duration = u64::try_from(duration).expect("can't narrow duration to u64");
        SampleDuration(duration)
    }

    pub fn to_std_duration_lossy(&self) -> core::time::Duration {
        core::time::Duration::from_micros(self.to_micros_lossy())
    }

    pub fn to_micros_lossy(&self) -> u64 {
        let usecs = (u128::from(self.0) * 1_000_000) / u128::from(SAMPLE_RATE);
        u64::try_from(usecs).expect("can't narrow usecs to u64")
    }

    pub fn add(&self, other: SampleDuration) -> Self {
        SampleDuration(self.0.checked_add(other.0).unwrap())
    }

    pub fn sub(&self, other: SampleDuration) -> Self {
        SampleDuration(self.0.checked_sub(other.0).expect("SampleDuration::sub would underflow!"))
    }
}

/// A duration with denominator SAMPLE_RATE, but it's signed :)
#[derive(Debug, Copy, Clone)]
pub struct TimestampDelta(i64);

impl TimestampDelta {
    pub fn zero() -> TimestampDelta {
        TimestampDelta(0)
    }

    pub fn abs(&self) -> SampleDuration {
        SampleDuration(u64::try_from(self.0.abs()).unwrap())
    }

    pub fn as_frames(&self) -> i64 {
        self.0
    }

    pub fn to_micros_lossy(&self) -> i64 {
        self.0 * 1_000_000 / i64::from(SAMPLE_RATE.0)
    }

    pub fn to_seconds(&self) -> f64 {
        self.0 as f64 / f64::from(SAMPLE_RATE)
    }
}
