mod audio;
mod config;
mod receive;
mod socket;
mod stats;
mod stream;
mod thread;
mod time;

use std::process::ExitCode;

use log::LevelFilter;
use structopt::StructOpt;
use thiserror::Error;

#[derive(StructOpt)]
enum Opt {
    Stream(stream::StreamOpt),
    Receive(receive::ReceiveOpt),
    Stats(stats::StatsOpt),
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error("opening network socket: {0}")]
    Listen(#[from] socket::ListenError),
    #[error("opening audio device: {0}")]
    OpenAudioDevice(#[from] audio::config::OpenError),
    #[error("receiving from network: {0}")]
    Receive(std::io::Error),
    #[error("opening encoder: {0}")]
    OpenEncoder(#[from] bark_core::encode::NewEncoderError),
}

fn main() -> Result<(), ExitCode> {
    init_log();

    if let Some(config) = config::read() {
        config::load_into_env(&config);
    }

    let opt = Opt::from_args();

    let result = match opt {
        Opt::Stream(opt) => stream::run(opt),
        Opt::Receive(opt) => receive::run(opt),
        Opt::Stats(opt) => stats::run(opt),
    };

    result.map_err(|err| {
        log::error!("fatal: {err}");
        ExitCode::FAILURE
    })
}

fn init_log() {
    env_logger::builder()
        .format_timestamp_millis()
        .filter_level(default_log_level())
        .parse_default_env()
        .init();
}

fn default_log_level() -> LevelFilter {
    if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    }
}
