use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::Router;
use axum::routing::get;
use structopt::StructOpt;
use thiserror::Error;

use crate::config;

#[derive(StructOpt)]
pub struct MetricsOpt {
    #[structopt(
        long,
        env = "BARK_METRICS_LISTEN",
        default_value = "0.0.0.0:1530",
    )]
    listen: SocketAddr,
}

pub struct MetricsServer {
    data: Arc<MetricsData>,
}

#[derive(Default)]
struct MetricsData {

}

#[derive(Debug, Error)]
#[error("starting metrics server: {0}")]
pub struct StartError(#[from] tokio::io::Error);

pub async fn start(opt: &MetricsOpt) -> Result<MetricsServer, StartError> {
    let data = Arc::new(MetricsData::default());

    let app = Router::new()
        .with_state(data.clone())
        .route("/metrics", get(metrics));

    let listener = tokio::net::TcpListener::bind(&opt.listen).await?;

    Ok(MetricsServer { data })
}

async fn metrics(data: State<Arc<MetricsData>>) -> String {
    format!("metrics")
}
