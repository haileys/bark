use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Router;
use axum::routing::get;
use derive_more::Deref;
use structopt::StructOpt;
use thiserror::Error;

use bark_protocol::time::{SampleDuration, TimestampDelta};

use super::value::{Counter, Gauge};

#[derive(StructOpt)]
pub struct MetricsOpt {
    #[structopt(
        long = "metrics-listen",
        env = "BARK_METRICS_LISTEN",
        default_value = "0.0.0.0:1530",
    )]
    listen: SocketAddr,
}

#[derive(Clone, Deref)]
pub struct MetricsSender {
    metrics: Arc<ReceiverMetrics>,
}

pub struct ReceiverMetrics {
    pub audio_offset: Gauge<Option<TimestampDelta>>,
    pub buffer_delay: Gauge<SampleDuration>,
    pub buffer_underruns: Counter,
    pub queued_packets: Gauge<usize>,
    pub network_latency: Gauge<Duration>,
    pub packets_received: Counter,
    pub packets_lost: Counter,
    pub packets_missed: Counter,
    pub frames_decoded: Counter,
    pub frames_played: Counter,
}

impl ReceiverMetrics {
    fn new() -> Self {
        ReceiverMetrics {
            audio_offset: Gauge::new("bark_receiver_audio_offset_usec"),
            buffer_delay: Gauge::new("bark_receiver_buffer_delay_usec"),
            buffer_underruns: Counter::new("bark_receiver_buffer_underruns"),
            network_latency: Gauge::new("bark_receiver_network_latency_usec"),
            queued_packets: Gauge::new("bark_receiver_queued_packet_count"),
            packets_received: Counter::new("bark_receiver_packets_received"),
            packets_lost: Counter::new("bark_receiver_packets_lost"),
            packets_missed: Counter::new("bark_receiver_packets_missed"),
            frames_decoded: Counter::new("bark_receiver_frames_decoded"),
            frames_played: Counter::new("bark_receiver_frames_played"),
        }
    }
}

#[derive(Debug, Error)]
#[error("starting metrics server: {0}")]
pub struct StartError(#[from] tokio::io::Error);

pub async fn start(opt: &MetricsOpt) -> Result<MetricsSender, StartError> {
    let metrics_data = Arc::new(ReceiverMetrics::new());

    let app = Router::new()
        .route("/metrics", get(metrics))
        .with_state(metrics_data.clone());

    let listener = tokio::net::TcpListener::bind(&opt.listen).await?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap()
    });

    Ok(MetricsSender { metrics: metrics_data })
}

async fn metrics(metrics: State<Arc<ReceiverMetrics>>) -> String {
    render_metrics(&metrics).unwrap_or_default()
}

fn render_metrics(metrics: &ReceiverMetrics) -> Result<String, std::fmt::Error> {
    let mut buffer = String::new();
    write!(&mut buffer, "{}", metrics.audio_offset)?;
    write!(&mut buffer, "{}", metrics.buffer_delay)?;
    write!(&mut buffer, "{}", metrics.buffer_underruns)?;
    write!(&mut buffer, "{}", metrics.network_latency)?;
    write!(&mut buffer, "{}", metrics.queued_packets)?;
    write!(&mut buffer, "{}", metrics.packets_received)?;
    write!(&mut buffer, "{}", metrics.packets_lost)?;
    write!(&mut buffer, "{}", metrics.packets_missed)?;
    write!(&mut buffer, "{}", metrics.frames_decoded)?;
    write!(&mut buffer, "{}", metrics.frames_played)?;
    Ok(buffer)
}
