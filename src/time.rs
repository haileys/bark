use crate::protocol::{self, TimestampMicros, TimePacket};

/// A timestamp with implicit denominator SAMPLE_RATE
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn now() -> Timestamp {
        Timestamp::from_micros_lossy(TimestampMicros::now())
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
    pub const ONE_PACKET: SampleDuration = SampleDuration::from_frame_count(protocol::FRAMES_PER_PACKET as u64);

    pub const fn zero() -> Self {
        SampleDuration(0)
    }

    pub const fn from_frame_count(samples: u64) -> Self {
        SampleDuration(samples)
    }

    pub fn from_std_duration_lossy(duration: std::time::Duration) -> SampleDuration {
        let duration = (duration.as_micros() * u128::from(protocol::SAMPLE_RATE.0)) / 1_000_000;
        let duration = u64::try_from(duration).expect("can't narrow duration to u64");
        SampleDuration(duration)
    }

    pub fn to_std_duration_lossy(&self) -> std::time::Duration {
        let usecs = (u128::from(self.0) * 1_000_000) / u128::from(protocol::SAMPLE_RATE.0);
        let usecs = u64::try_from(usecs).expect("can't narrow usecs to u64");
        std::time::Duration::from_micros(usecs)
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

    pub fn add(&self, other: SampleDuration) -> Self {
        SampleDuration(self.0.checked_add(other.0).unwrap())
    }

    pub fn sub(&self, other: SampleDuration) -> Self {
        SampleDuration(self.0.checked_sub(other.0).expect("SampleDuration::sub would underflow!"))
    }
}

/// The difference between two machine clocks in microseconds
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord, Default)]
pub struct ClockDelta(i64);

impl ClockDelta {
    pub fn as_micros(&self) -> i64 {
        self.0
    }

    /// Calculates clock difference between machines based on a complete TimePacket
    pub fn from_time_packet(packet: &TimePacket) -> ClockDelta {
        // all fields should be non-zero here, it's a programming error if
        // they're not.
        assert!(packet.stream_1.0 != 0);
        assert!(packet.receive_2.0 != 0);
        assert!(packet.stream_3.0 != 0);

        let t1_usec = packet.stream_1.0 as i64;
        let t2_usec = packet.receive_2.0 as i64;
        let t3_usec = packet.stream_3.0 as i64;

        // algorithm from the Precision Time Protocol page on Wikipedia
        ClockDelta((t2_usec - t1_usec + t2_usec - t3_usec) / 2)
    }
}

/// A duration with denominator SAMPLE_RATE, but it's signed :)
#[derive(Debug, Copy, Clone)]
pub struct TimestampDelta(i64);

impl TimestampDelta {
    pub fn from_clock_delta_lossy(delta: ClockDelta) -> TimestampDelta {
        TimestampDelta((delta.0 * i64::from(protocol::SAMPLE_RATE.0)) / 1_000_000)
    }

    pub fn abs(&self) -> SampleDuration {
        SampleDuration(u64::try_from(self.0.abs()).unwrap())
    }

    pub fn as_frames(&self) -> i64 {
        self.0
    }
}
