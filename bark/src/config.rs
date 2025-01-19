use std::env;
use std::net::SocketAddr;
use std::path::Path;

use derive_more::{Display, FromStr};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    multicast: Option<SocketAddr>,
    #[serde(default)]
    source: Source,
    #[serde(default)]
    receive: Receive,
    #[serde(default)]
    metrics: Metrics,
}

#[derive(Deserialize, Default)]
pub struct Source {
    #[serde(default)]
    input: Device,
    delay_ms: Option<u64>,
    codec: Option<Codec>,
}

#[derive(Deserialize, Default)]
pub struct Metrics {
    listen: Option<SocketAddr>,
}

#[derive(Deserialize, Display, FromStr, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[display("s16le")]
    S16LE,
    #[display("f32le")]
    F32LE,
    #[cfg(feature = "opus")]
    #[display("opus")]
    Opus,
}

#[derive(Deserialize, Default)]
pub struct Receive {
    #[serde(default)]
    output: Device,
}

#[derive(Deserialize, Default)]
pub struct Device {
    device: Option<String>,
    period: Option<u64>,
    buffer: Option<u64>,
    format: Option<Format>,
}

#[derive(Deserialize, Display, FromStr, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[display("s16")]
    S16,
    #[display("f32")]
    F32,
}

fn set_env<T: ToString>(name: &str, value: T) {
    env::set_var(name, value.to_string());
}

fn set_env_option<T: ToString>(name: &str, value: Option<T>) {
    if let Some(value) = value {
        set_env(name, value)
    }
}

pub fn load_into_env(config: &Config) {
    set_env_option("BARK_MULTICAST", config.multicast);
    set_env_option("BARK_SOURCE_DELAY_MS", config.source.delay_ms);
    set_env_option("BARK_SOURCE_INPUT_DEVICE", config.source.input.device.as_ref());
    set_env_option("BARK_SOURCE_INPUT_PERIOD", config.source.input.period);
    set_env_option("BARK_SOURCE_INPUT_BUFFER", config.source.input.buffer);
    set_env_option("BARK_SOURCE_INPUT_FORMAT", config.source.input.format);
    set_env_option("BARK_SOURCE_CODEC", config.source.codec);
    set_env_option("BARK_RECEIVE_OUTPUT_DEVICE", config.receive.output.device.as_ref());
    set_env_option("BARK_RECEIVE_OUTPUT_PERIOD", config.receive.output.period);
    set_env_option("BARK_RECEIVE_OUTPUT_BUFFER", config.receive.output.buffer);
    set_env_option("BARK_RECEIVE_OUTPUT_FORMAT", config.receive.output.format);
    set_env_option("BARK_METRICS_LISTEN", config.metrics.listen);
}

fn load_file(path: &Path) -> Option<Config> {
    log::debug!("looking for config in {}", path.display());

    let contents = std::fs::read_to_string(path).ok()?;

    match toml::from_str(&contents) {
        Ok(config) => {
            log::info!("reading config from {}", path.display());
            Some(config)
        },
        Err(e) => {
            log::error!("error reading config: {}", e);
            std::process::exit(1);
        }
    }
}

pub fn read() -> Option<Config> {
    // try current directory first
    if let Some(config) = load_file(Path::new("bark.toml")) {
        return Some(config);
    }

    // otherwise try xdg config dirs
    let dirs = xdg::BaseDirectories::new().unwrap();
    if let Some(config) = dirs.find_config_file("bark.toml") {
        return load_file(&config);
    }

    // found nothing
    None
}
