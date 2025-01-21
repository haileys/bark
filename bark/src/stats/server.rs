use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::Router;
use axum::routing::get;
use structopt::StructOpt;
use thiserror::Error;

use super::metrics::{ReceiverMetrics, ReceiverMetricsData, SourceMetrics, SourceMetricsData};

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
enum MetricsState {
    Receiver(ReceiverMetrics),
    Source(SourceMetrics),
}

#[derive(Debug, Error)]
#[error("starting metrics server: {0}")]
pub struct StartError(#[from] tokio::io::Error);

pub async fn start_receiver(opt: &MetricsOpt) -> Result<ReceiverMetrics, StartError> {
    let metrics = Arc::new(ReceiverMetricsData::new());
    start(opt, MetricsState::Receiver(metrics.clone())).await?;
    Ok(metrics)
}

pub async fn start_source(opt: &MetricsOpt) -> Result<SourceMetrics, StartError> {
    let metrics = Arc::new(SourceMetricsData::new());
    start(opt, MetricsState::Source(metrics.clone())).await?;
    Ok(metrics)
}

async fn start(opt: &MetricsOpt, state: MetricsState) -> Result<(), StartError> {
    let app = Router::new()
        .route("/metrics", get(metrics))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&opt.listen).await?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap()
    });

    Ok(())
}

async fn metrics(metrics: State<MetricsState>) -> String {
    match &*metrics {
        MetricsState::Receiver(metrics) => render_receiver_metrics(metrics).unwrap_or_default(),
        MetricsState::Source(metrics) => render_source_metrics(metrics).unwrap_or_default(),
    }
}

fn render_receiver_metrics(metrics: &ReceiverMetrics) -> Result<String, std::fmt::Error> {
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

fn render_source_metrics(_metrics: &SourceMetrics) -> Result<String, std::fmt::Error> {
    let buffer = String::new();
    Ok(buffer)
}
