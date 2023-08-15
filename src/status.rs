use std::{io::Write, time::{Instant, Duration}};

use termcolor::{BufferedStandardStream, WriteColor, ColorSpec, Color};

use crate::time::{Timestamp, ClockDelta, SampleDuration};

const RENDER_INTERVAL: Duration = Duration::from_millis(32);

pub struct Status {
    sync: bool,
    audio_latency_sec: Option<f64>,
    buffer_length_sec: Option<f64>,
    network_latency_sec: Option<f64>,
    clock_delta_sec: Option<f64>,
    last_render: Option<Instant>,
}

impl Status {
    pub fn new() -> Self {
        Status {
            sync: false,
            audio_latency_sec: None,
            buffer_length_sec: None,
            network_latency_sec: None,
            clock_delta_sec: None,
            last_render: None,
        }
    }

    pub fn set_sync(&mut self) {
        self.sync = true;
    }

    pub fn clear_sync(&mut self) {
        self.sync = false;
        self.audio_latency_sec = None;
        self.buffer_length_sec = None;
        self.network_latency_sec = None;
        self.clock_delta_sec = None;
    }

    pub fn record_audio_latency(&mut self, request_pts: Timestamp, packet_pts: Timestamp) {
        let request_micros = request_pts.to_micros_lossy().0 as f64;
        let packet_micros = packet_pts.to_micros_lossy().0 as f64;

        self.audio_latency_sec = Some((packet_micros - request_micros) / 1_000_000.0);
    }

    pub fn record_buffer_length(&mut self, length: SampleDuration) {
        self.buffer_length_sec = Some(length.to_std_duration_lossy().as_micros() as f64 / 1_000_000.0);
    }

    pub fn record_network_latency(&mut self, latency: Duration) {
        self.network_latency_sec = Some(latency.as_micros() as f64 / 1_000_000.0);
    }

    pub fn record_clock_delta(&mut self, delta: ClockDelta) {
        self.clock_delta_sec = Some(delta.as_micros() as f64 / 1_000_000.0);
    }

    pub fn render(&mut self) {
        let now = Instant::now();

        if let Some(instant) = self.last_render {
            let duration = now.duration_since(instant);
            if duration < RENDER_INTERVAL {
                return;
            }
        }

        self.last_render = Some(now);

        let mut out = BufferedStandardStream::stdout(termcolor::ColorChoice::Auto);

        let _ = write!(&mut out, "\r");

        if self.sync {
            let _ = out.set_color(&ColorSpec::new()
                .set_bg(Some(Color::Green))
                .set_fg(Some(Color::Rgb(0, 0, 0))) // dark black
                .set_bold(true)
                .set_intense(true));

            let _ = out.write_all(b"  SYNC  ");

            let _ = out.set_color(&ColorSpec::new());
        } else {
            let _ = out.set_color(&ColorSpec::new()
                .set_dimmed(true));

            let _ = out.write_all(b" UNSYNC ");
        }

        if let Some(latency_sec) = self.audio_latency_sec {
            let _ = write!(&mut out, "  Audio:[{:>8.3} ms]", latency_sec * 1000.0);
        } else {
            let _ = write!(&mut out, "  Audio:[        ms]");
        }

        if let Some(buffer_sec) = self.buffer_length_sec {
            let _ = write!(&mut out, "  Buffer:[{:>8.3} ms]", buffer_sec * 1000.0);
        } else {
            let _ = write!(&mut out, "  Buffer:[        ms]");
        }

        if let Some(latency_sec) = self.network_latency_sec {
            let _ = write!(&mut out, "  Network:[{:>8.3} ms]", latency_sec * 1000.0);
        } else {
            let _ = write!(&mut out, "  Network:[        ms]");
        }

        if let Some(delta_sec) = self.clock_delta_sec {
            let _ = write!(&mut out, "  Clock:[{:>8.3} ms]", delta_sec * 1000.0);
        } else {
            let _ = write!(&mut out, "  Clock:[        ms]");
        }

        let _ = write!(&mut out, " ");

        let _ = out.flush();
    }
}
