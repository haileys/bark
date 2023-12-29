use nix::sys::time::TimeValLike;
use nix::time::ClockId;

use bark_protocol::types::TimestampMicros;

pub fn now() -> TimestampMicros {
    let timespec = nix::time::clock_gettime(ClockId::CLOCK_MONOTONIC_RAW)
        .expect("clock_gettime(CLOCK_MONOTONIC_RAW) failed, are we on Linux?");

    let micros = u64::try_from(timespec.num_microseconds())
        .expect("cannot convert i64 time value to u64");

    TimestampMicros(micros)
}
