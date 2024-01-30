use std::env;
use std::fmt::Display;
use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

#[derive(Deserialize)]
pub struct Config {
    multicast: Option<SocketAddr>,
    #[serde(default)]
    source: Source,
    #[serde(default)]
    receive: Receive,
}

#[derive(Deserialize, Default)]
pub struct Source {
    #[serde(default)]
    input: Device,
    delay_ms: Option<u64>,
    format: Option<StreamFormat>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamFormat {
    S16LE,
    F32LE,
    Opus,
}

#[derive(Debug, Error)]
#[error("unknown format")]
pub struct UnknownFormat;

impl FromStr for StreamFormat {
    type Err = UnknownFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s16le" => Ok(StreamFormat::S16LE),
            "f32le" => Ok(StreamFormat::F32LE),
            "opus" => Ok(StreamFormat::Opus),
            _ => Err(UnknownFormat),
        }
    }
}

impl Display for StreamFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamFormat::S16LE => write!(f, "s16le"),
            StreamFormat::F32LE => write!(f, "f32le"),
            StreamFormat::Opus => write!(f, "opus"),
        }
    }
}

#[derive(Deserialize, Debug, Copy, Clone)]
#[serde(try_from = "String")]
pub enum SampleFormat {
    S16,
    F32,
}

impl FromStr for SampleFormat {
    type Err = UnknownFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s16" => Ok(SampleFormat::S16),
            "f32" => Ok(SampleFormat::F32),
            _ => Err(UnknownFormat),
        }
    }
}

impl TryFrom<String> for SampleFormat {
    type Error = <Self as FromStr>::Err;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Display for SampleFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SampleFormat::S16 => write!(f, "s16"),
            SampleFormat::F32 => write!(f, "f32"),
        }
    }
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
    format: Option<SampleFormat>,
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
    set_env_option("BARK_SOURCE_FORMAT", config.source.format.as_ref());
    set_env_option("BARK_RECEIVE_OUTPUT_DEVICE", config.receive.output.device.as_ref());
    set_env_option("BARK_RECEIVE_OUTPUT_PERIOD", config.receive.output.period);
    set_env_option("BARK_RECEIVE_OUTPUT_BUFFER", config.receive.output.buffer);
    set_env_option("BARK_RECEIVE_OUTPUT_FORMAT", config.receive.output.format);
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
