mod config;
mod receive;
mod resample;
mod stats;
mod stream;
mod time;

use std::process::ExitCode;

use structopt::StructOpt;

#[derive(StructOpt)]
enum Opt {
    Stream(stream::StreamOpt),
    Receive(receive::ReceiveOpt),
    Stats(stats::StatsOpt),
}

#[derive(StructOpt, Debug, Clone)]
struct SocketOpt {
    #[structopt(long, name="addr", env = "BARK_MULTICAST")]
    /// Multicast group address including port, eg. 224.100.100.100:1530
    pub multicast: std::net::SocketAddrV4,
}

#[derive(Debug)]
pub enum RunError {
    Listen(bark_network::ListenError),
    NoDeviceAvailable,
    ConfigureDevice(bark_device::util::ConfigError),
    BuildStream(cpal::BuildStreamError),
    Stream(cpal::PlayStreamError),
    Socket(std::io::Error),
}

fn main() -> Result<(), ExitCode> {
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
        eprintln!("error: {err:?}");
        ExitCode::FAILURE
    })
}
