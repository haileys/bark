mod protocol;
mod receive;
mod resample;
mod status;
mod stream;
mod time;
mod util;

use std::net::SocketAddrV4;
use std::process::ExitCode;

use structopt::StructOpt;

#[derive(StructOpt)]
enum Opt {
    Stream(stream::StreamOpt),
    Receive(receive::ReceiveOpt),
}

#[derive(Debug)]
pub enum RunError {
    BindSocket(SocketAddrV4, std::io::Error),
    JoinMulticast(std::io::Error),
    NoDeviceAvailable,
    NoSupportedStreamConfig,
    StreamConfigs(cpal::SupportedStreamConfigsError),
    BuildStream(cpal::BuildStreamError),
    Stream(cpal::PlayStreamError),
    Socket(std::io::Error),
}

fn main() -> Result<(), ExitCode> {
    let opt = Opt::from_args();

    let result = match opt {
        Opt::Stream(opt) => stream::run(opt),
        Opt::Receive(opt) => receive::run(opt),
    };

    result.map_err(|err| {
        eprintln!("error: {err:?}");
        ExitCode::FAILURE
    })
}
