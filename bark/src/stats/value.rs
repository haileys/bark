use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

use bark_core::audio::FrameCount;
use bark_protocol::time::{SampleDuration, TimestampDelta};

pub struct Counter {
    name: &'static str,
    value: AtomicU64,
}

impl Counter {
    pub fn new(name: &'static str) -> Self {
        Counter { name, value: AtomicU64::new(0) }
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn add(&self, n: usize) {
        let n = u64::try_from(n).unwrap_or_default();
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    pub fn increment(&self) {
        self.add(1);
    }
}

impl Display for Counter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "# TYPE {} counter\n", self.name)?;
        write!(f, "{} {}\n\n", self.name, self.get())?;
        Ok(())
    }
}

const GAUGE_NO_VALUE: i64 = i64::MIN;

pub struct Gauge<T> {
    name: &'static str,
    value: AtomicI64,
    _phantom: PhantomData<T>,
}

impl<T> Gauge<T> where T: GaugeValue {
    pub fn new(name: &'static str) -> Self {
        Gauge {
            name,
            value: AtomicI64::new(GAUGE_NO_VALUE),
            _phantom: PhantomData,
        }
    }

    pub fn get(&self) -> Option<i64> {
        Some(self.value.load(Ordering::Relaxed))
            .filter(|val| *val != GAUGE_NO_VALUE)
    }

    pub fn observe(&self, value: T) {
        self.value.store(value.to_i64(), Ordering::Relaxed);
    }
}

impl<T> Display for Gauge<T> where T: GaugeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(value) = self.get() {
            write!(f, "# TYPE {} gauge\n", self.name)?;
            write!(f, "{} {}", self.name, value)?;
        }
        Ok(())
    }
}

pub trait GaugeValue {
    fn to_i64(&self) -> i64;
}

impl<T> GaugeValue for Option<T> where T: GaugeValue {
    fn to_i64(&self) -> i64 {
        match self {
            None => GAUGE_NO_VALUE,
            // just ignore semi-predicate problem here:
            Some(val) => val.to_i64(),
        }
    }
}

impl GaugeValue for usize {
    fn to_i64(&self) -> i64 {
        i64::try_from(*self).unwrap_or(GAUGE_NO_VALUE)
    }
}

impl GaugeValue for TimestampDelta {
    fn to_i64(&self) -> i64 {
        self.to_micros_lossy()
    }
}

impl GaugeValue for SampleDuration {
    fn to_i64(&self) -> i64 {
        i64::try_from(self.to_micros_lossy()).unwrap_or(GAUGE_NO_VALUE)
    }
}

impl GaugeValue for Duration {
    fn to_i64(&self) -> i64 {
        i64::try_from(self.as_micros()).unwrap_or(GAUGE_NO_VALUE)
    }
}

impl GaugeValue for FrameCount {
    fn to_i64(&self) -> i64 {
        i64::try_from(self.0).unwrap_or(GAUGE_NO_VALUE)
    }
}
