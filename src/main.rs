mod protocol;
mod receive;
mod resample;
mod status;
mod stream;
mod time;
mod util;

use std::net::{UdpSocket, Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;
use std::sync::{Mutex, Arc};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{OutputCallbackInfo};
use structopt::StructOpt;

use protocol::TimestampMicros;

use crate::protocol::Packet;
use crate::time::{SampleDuration, Timestamp};

#[derive(StructOpt)]
enum Opt {
    Stream(stream::StreamOpt),
    Receive(ReceiveOpt),
}

#[derive(StructOpt, Clone)]
struct ReceiveOpt {
    #[structopt(long, short)]
    pub group: Ipv4Addr,
    #[structopt(long, short)]
    pub port: u16,
    #[structopt(long, short)]
    pub bind: Option<Ipv4Addr>,
    #[structopt(long, default_value="12")]
    pub max_seq_gap: usize,
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
        Opt::Receive(opt) => run_receive(opt),
    };

    result.map_err(|err| {
        eprintln!("error: {err:?}");
        ExitCode::FAILURE
    })
}

fn run_receive(opt: ReceiveOpt) -> Result<(), RunError> {
    let host = cpal::default_host();

    let device = host.default_output_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = util::config_for_device(&device)?;

    struct SharedState {
        pub recv: receive::Receiver,
    }

    let state = Arc::new(Mutex::new(SharedState {
        recv: receive::Receiver::new(receive::ReceiverOpt {
            max_seq_gap: opt.max_seq_gap,
        }),
    }));

    let _stream = device.build_output_stream(&config,
        {
            let state = state.clone();
            move |data: &mut [f32], info: &OutputCallbackInfo| {
                let stream_timestamp = info.timestamp();

                let output_latency = stream_timestamp.playback
                    .duration_since(&stream_timestamp.callback)
                    .unwrap_or_default();

                let output_latency = SampleDuration::from_std_duration_lossy(output_latency);

                let now = Timestamp::now();
                let pts = now.add(output_latency);

                let mut state = state.lock().unwrap();
                state.recv.fill_stream_buffer(data, pts);
            }
        },
        move |err| {
            eprintln!("stream error! {err:?}");
        },
        None
    ).map_err(RunError::BuildStream)?;

    let bind_ip = opt.bind.unwrap_or(Ipv4Addr::UNSPECIFIED);
    let bind_addr = SocketAddrV4::new(bind_ip, opt.port);

    let socket = UdpSocket::bind(bind_addr)
        .map_err(|e| RunError::BindSocket(bind_addr, e))?;

    socket.join_multicast_v4(&opt.group, &bind_ip)
        .map_err(RunError::JoinMulticast)?;

    util::set_expedited_forwarding(&socket);

    loop {
        let mut packet_raw = [0u8; protocol::MAX_PACKET_SIZE];

        let (nbytes, addr) = socket.recv_from(&mut packet_raw)
            .map_err(RunError::Socket)?;

        match Packet::try_from_bytes_mut(&mut packet_raw[0..nbytes]) {
            Some(Packet::Time(time)) => {
                if time.stream_3.0 == 0 {
                    // we need to respond to this packet
                    time.receive_2 = TimestampMicros::now();
                    socket.send_to(bytemuck::bytes_of(time), addr)
                        .expect("reply to time packet");
                    continue;
                }

                let mut state = state.lock().unwrap();
                state.recv.receive_time(time);
            }
            Some(Packet::Audio(packet)) => {
                let mut state = state.lock().unwrap();
                state.recv.receive_audio(packet);
            }
            None => {
                // unknown packet type, ignore
            }
        }
    }
}
