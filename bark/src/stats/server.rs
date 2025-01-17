use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;
use std::u64;

use axum::extract::State;
use axum::Router;
use axum::routing::get;
use bark_protocol::time::{SampleDuration, TimestampDelta};
use structopt::StructOpt;
use thiserror::Error;

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
    pub fn observe_audio_offset(&self, delta: TimestampDelta) {
        let value = delta.to_micros_lossy();
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
}

#[derive(Default)]
struct MetricsData {
    audio_offset: AtomicI64,
    buffer_length: AtomicU64,
    network_latency: AtomicU64,
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
    let mut out = String::new();
    write!(&mut out, "bark_receiver_audio_offset_usec {}\n", data.audio_offset.load(Ordering::Relaxed))?;
    write!(&mut out, "bark_receiver_buffer_length_usec {}\n", data.buffer_length.load(Ordering::Relaxed))?;
    write!(&mut out, "bark_receiver_network_latency_usec {}\n", data.network_latency.load(Ordering::Relaxed))?;
    Ok(out)
}
