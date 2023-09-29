mod config;
mod receive;
mod stats;
mod source;

use std::process::ExitCode;

use derive_more::From;
use structopt::StructOpt;

#[derive(StructOpt)]
enum Opt {
    Stream(source::StreamOpt),
    Receive(receive::ReceiveOpt),
    Stats(stats::StatsOpt),
}

#[derive(StructOpt, Debug, Clone)]
struct SocketOpt {
    #[structopt(long, name="addr", env = "BARK_MULTICAST")]
    /// Multicast group address including port, eg. 224.100.100.100:1530
    pub multicast: std::net::SocketAddrV4,
}

#[derive(Debug, From)]
pub enum RunError {
    Listen(bark_network::ListenError),
    NoDeviceAvailable,
    OpenDevice(bark_device::OpenError),
    StartDecode(bark_core::decode::NewDecodeError),
    BuildStream(cpal::BuildStreamError),
    Stream(cpal::PlayStreamError),
    Socket(std::io::Error),
}

fn main() -> Result<(), ExitCode> {
    pretty_env_logger::formatted_timed_builder()
        .filter_level(default_log_level())
        .parse_default_env()
        .init();

    if let Some(config) = config::read() {
        config::load_into_env(&config);
    }

    let opt = Opt::from_args();

    let result = match opt {
        Opt::Stream(opt) => source::run(opt),
        Opt::Receive(opt) => receive::run(opt),
        Opt::Stats(opt) => stats::run(opt),
    };

    result.map_err(|err| {
        log::error!("{err:?}");
        ExitCode::FAILURE
    })
}

fn default_log_level() -> log::LevelFilter {
    if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    }
}
