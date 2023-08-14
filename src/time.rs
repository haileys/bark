use std::time::SystemTime;

use crate::protocol::{self, TimestampMicros};

/// A timestamp with implicit denominator SAMPLE_RATE
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn now() -> Timestamp {
        // SystemTime::now uses CLOCK_REALTIME on Linux, which is exactly what we want
        // https://doc.rust-lang.org/std/time/struct.SystemTime.html#platform-specific-behavior
        let micros = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime::now before UNIX_EPOCH!")
            .as_micros();

        let micros = u64::try_from(micros)
            .expect("can't narrow timestamp to u64");

        Timestamp::from_micros_lossy(TimestampMicros(micros))
    }
}

impl Timestamp {
    pub fn to_micros_lossy(&self) -> TimestampMicros {
        let ts = u128::from(self.0);
        let micros = (ts * 1_000_000) / u128::from(protocol::SAMPLE_RATE.0);
        let micros = u64::try_from(micros)
            .expect("can't narrow timestamp to u64");
        TimestampMicros(micros)
    }

    pub fn from_micros_lossy(micros: TimestampMicros) -> Timestamp {
        let micros = u128::from(micros.0);
        let ts = (micros * u128::from(protocol::SAMPLE_RATE.0)) / 1_000_000;
        let ts = u64::try_from(ts)
            .expect("can't narrow timestamp to u64");
        Timestamp(ts)
    }

    pub fn add(&self, duration: SampleDuration) -> Timestamp {
        Timestamp(self.0.checked_add(duration.0).unwrap())
    }

    pub fn sub(&self, duration: SampleDuration) -> Timestamp {
        Timestamp(self.0.checked_sub(duration.0).unwrap())
    }

    pub fn duration_since(&self, other: Timestamp) -> SampleDuration {
        SampleDuration(self.0.checked_sub(other.0).unwrap())
    }
}

/// A duration with implicit denominator SAMPLE_RATE
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleDuration(u64);

impl SampleDuration {
    pub const ONE_PACKET: SampleDuration = SampleDuration::from_sample_count(protocol::FRAMES_PER_PACKET as u64);

    pub const fn zero() -> Self {
        SampleDuration(0)
    }

    pub const fn from_sample_count(samples: u64) -> Self {
        SampleDuration(samples)
    }

    pub fn from_std_duration_lossy(duration: std::time::Duration) -> SampleDuration {
        let duration = duration.as_micros() * u128::from(protocol::SAMPLE_RATE.0) / 1_000_000;
        let duration = u64::try_from(duration).expect("can't narrow duration to u64");
        SampleDuration(duration)
    }

    pub fn to_std_duration_lossy(&self) -> std::time::Duration {
        let micros = (u128::from(self.0) * 1_000_000) / u128::from(protocol::SAMPLE_RATE.0);
        let micros = u64::try_from(micros).expect("can't narrow durection to u64");
        std::time::Duration::from_micros(micros)
    }

    pub fn mul(&self, times: u64) -> Self {
        SampleDuration(self.0.checked_mul(times).unwrap())
    }

    pub fn as_buffer_offset(&self) -> usize {
        let offset = self.0 * u64::from(protocol::CHANNELS);
        usize::try_from(offset).unwrap()
    }

    pub fn from_buffer_offset(offset: usize) -> Self {
        let channels = usize::from(protocol::CHANNELS);
        assert!(offset % channels == 0);

        SampleDuration(u64::try_from(offset / channels).unwrap())
    }
}
