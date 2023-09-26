use std::env;
use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;

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
    device: Option<String>,
    delay_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
pub struct Receive {
    device: Option<String>,
}

fn set_env_option<T: ToString>(name: &str, value: Option<T>) {
    if let Some(value) = value {
        env::set_var(name, value.to_string());
    }
}

pub fn load_into_env(config: &Config) {
    set_env_option("BARK_MULTICAST", config.multicast);
    set_env_option("BARK_SOURCE_DEVICE", config.source.device.as_ref());
    set_env_option("BARK_SOURCE_DELAY_MS", config.source.delay_ms);
    set_env_option("BARK_RECEIVE_DEVICE", config.receive.device.as_ref());
}

fn load_file(path: &Path) -> Option<Config> {
    let contents = std::fs::read_to_string(path).ok()?;

    eprintln!("reading config from {}", path.display());

    match toml::from_str(&contents) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("error reading config: {}", e);
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
