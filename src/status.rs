use std::{io::Write, time::{Instant, Duration}};

use termcolor::{BufferedStandardStream, WriteColor, ColorSpec, Color};

use crate::time::Timestamp;

const RENDER_INTERVAL: Duration = Duration::from_millis(16);

pub struct Status {
    sync: bool,
    latency_sec: Option<f64>,
    last_render: Option<Instant>,
}

impl Status {
    pub fn new() -> Self {
        Status {
            sync: false,
            latency_sec: None,
            last_render: None,
        }
    }

    pub fn set_sync(&mut self) {
        self.sync = true;
    }

    pub fn clear_sync(&mut self) {
        self.sync = false;
        self.latency_sec = None;
    }

    pub fn record_latency(&mut self, request_pts: Timestamp, packet_pts: Timestamp) {
        let request_micros = request_pts.to_micros_lossy().0 as f64;
        let packet_micros = packet_pts.to_micros_lossy().0 as f64;

        self.latency_sec = Some((packet_micros - request_micros) / 1_000_000.0);
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

        if let Some(latency_sec) = self.latency_sec {
            let _ = write!(&mut out, " [{:>8.3} ms]", latency_sec * 1000.0);
        } else {
            let _ = write!(&mut out, " [        ms]");
        }

        let _ = write!(&mut out, " ");

        let _ = out.flush();
    }
}
