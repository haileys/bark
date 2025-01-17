use nix::sys::time::TimeValLike;
use nix::time::ClockId;

use bark_protocol::types::TimestampMicros;

pub fn now() -> TimestampMicros {
    let timespec = nix::time::clock_gettime(ClockId::CLOCK_REALTIME)
        .expect("clock_gettime(CLOCK_REALTIME)");

    let micros = u64::try_from(timespec.num_microseconds())
        .expect("cannot convert i64 time value to u64");

    TimestampMicros(micros)
}
