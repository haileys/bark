use std::fmt::{self, Display, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;
use std::{i64, u64};

use axum::extract::State;
use axum::Router;
use axum::routing::get;
use structopt::StructOpt;
use thiserror::Error;

use bark_core::audio::FrameCount;
use bark_protocol::time::{SampleDuration, TimestampDelta};

#[derive(StructOpt)]
pub struct MetricsOpt {
    #[structopt(
        long = "metrics-listen",
        env = "BARK_METRICS_LISTEN",
        default_value = "0.0.0.0:1530",
    )]
    listen: SocketAddr,
}

#[derive(Clone)]
pub struct MetricsSender {
    data: Arc<MetricsData>,
}

impl MetricsSender {
    pub fn observe_audio_offset(&self, delta: Option<TimestampDelta>) {
        let value = match delta {
            Some(delta) => delta.to_micros_lossy(),
            // i64::MIN is a sentinel value indicating missing value
            None => i64::MIN,
        };

        self.data.audio_offset.store(value, Ordering::Relaxed);
    }

    pub fn observe_buffer_length(&self, length: SampleDuration) {
        let value = length.to_micros_lossy();
        self.data.buffer_length.store(value, Ordering::Relaxed);
    }

    pub fn observe_network_latency(&self, latency: Duration) {
        let value = u64::try_from(latency.as_micros()).unwrap_or(u64::MAX);
        self.data.network_latency.store(value, Ordering::Relaxed);
    }

    pub fn increment_packets_received(&self) {
        self.data.packets_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_frames_decoded(&self, count: FrameCount) {
        let value = u64::try_from(count.0).expect("usize -> u64");
        self.data.frames_decoded.fetch_add(value, Ordering::Relaxed);
    }

    pub fn increment_frames_played(&self, count: FrameCount) {
        let value = u64::try_from(count.0).expect("usize -> u64");
        self.data.frames_played.fetch_add(value, Ordering::Relaxed);
    }
}

struct MetricsData {
    audio_offset: AtomicI64,
    buffer_length: AtomicU64,
    network_latency: AtomicU64,
    packets_received: AtomicU64,
    frames_decoded: AtomicU64,
    frames_played: AtomicU64,
}

impl Default for MetricsData {
    fn default() -> Self {
        MetricsData {
            audio_offset: AtomicI64::new(i64::MIN),
            buffer_length: Default::default(),
            network_latency: Default::default(),
            packets_received: Default::default(),
            frames_decoded: Default::default(),
            frames_played: Default::default(),
        }
    }
}

#[derive(Debug, Error)]
#[error("starting metrics server: {0}")]
pub struct StartError(#[from] tokio::io::Error);

pub async fn start(opt: &MetricsOpt) -> Result<MetricsSender, StartError> {
    let data = Arc::new(MetricsData::default());

    let app = Router::new()
        .route("/metrics", get(metrics))
        .with_state(data.clone());

    let listener = tokio::net::TcpListener::bind(&opt.listen).await?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap()
    });

    Ok(MetricsSender { data })
}

async fn metrics(data: State<Arc<MetricsData>>) -> String {
    render_metrics(&data).unwrap_or_default()
}

fn render_metrics(data: &MetricsData) -> Result<String, std::fmt::Error> {
    let mut render = RenderMetrics::new();

    let audio_offset_usec = data.audio_offset.load(Ordering::Relaxed);
    if audio_offset_usec != i64::MIN {
        render.gauge("bark_receiver_audio_offset_usec", audio_offset_usec)?;
    }

    render.gauge("bark_receiver_buffer_length_usec", data.buffer_length.load(Ordering::Relaxed))?;

    let network_latency_usec = data.network_latency.load(Ordering::Relaxed);
    if network_latency_usec != 0 {
        render.gauge("bark_receiver_network_latency_usec", network_latency_usec)?;
    }

    render.counter("bark_receiver_packets_received", data.packets_received.load(Ordering::Relaxed))?;
    render.counter("bark_receiver_frames_decoded", data.frames_decoded.load(Ordering::Relaxed))?;
    render.counter("bark_receiver_frames_played", data.frames_played.load(Ordering::Relaxed))?;
    Ok(render.finish())
}

struct RenderMetrics {
    buff: String,
}

impl RenderMetrics {
    pub fn new() -> Self {
        RenderMetrics { buff: String::new() }
    }

    fn expose(&mut self, type_: &str, name: &str, value: impl Display) -> fmt::Result {
        write!(&mut self.buff, "# TYPE {name} {type_}\n")?;
        write!(&mut self.buff, "{name} {value}\n")?;
        write!(&mut self.buff, "\n")?;
        Ok(())
    }

    pub fn gauge(&mut self, name: &str, value: impl Display) -> fmt::Result {
        self.expose("gauge", name, value)
    }

    pub fn counter(&mut self, name: &str, value: impl Display) -> fmt::Result {
        self.expose("counter", name, value)
    }

    pub fn finish(self) -> String {
        self.buff
    }
}
