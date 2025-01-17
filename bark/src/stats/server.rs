use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::Router;
use axum::routing::get;
use structopt::StructOpt;
use thiserror::Error;

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
        .route("/metrics", get(metrics))
        .with_state(data.clone());

    let listener = tokio::net::TcpListener::bind(&opt.listen).await?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap()
    });

    Ok(MetricsServer { data })
}

async fn metrics(data: State<Arc<MetricsData>>) -> String {
    format!("metrics")
}
