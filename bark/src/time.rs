use nix::sys::time::{TimeSpec, TimeValLike};
use nix::time::ClockId;

use bark_protocol::types::TimestampMicros;

pub fn now() -> TimestampMicros {
    let timespec = monotonic_time();

    let micros = u64::try_from(timespec.num_microseconds())
        .expect("cannot convert i64 time value to u64");

    TimestampMicros(micros)
}

#[cfg(target_os = "linux")]
fn monotonic_time() -> TimeSpec {
    nix::time::clock_gettime(ClockId::CLOCK_MONOTONIC_RAW).unwrap()
}

#[cfg(target_os = "macos")]
fn monotonic_time() -> TimeSpec {
    nix::time::clock_gettime(ClockId::from(libc::CLOCK_UPTIME_RAW)).unwrap()
}
