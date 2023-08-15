mod receive;
mod protocol;
mod time;
mod status;
mod resample;

use std::net::{UdpSocket, Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;
use std::sync::{Mutex, Arc};
use std::time::Duration;

use bytemuck::Zeroable;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{OutputCallbackInfo, StreamConfig, InputCallbackInfo, BufferSize, SupportedBufferSize};
use structopt::StructOpt;

use protocol::{TimestampMicros, AudioPacket, PacketBuffer, TimePacket, MAX_PACKET_SIZE};

use crate::protocol::Packet;
use crate::time::{SampleDuration, Timestamp};

#[derive(StructOpt)]
enum Opt {
    Stream(StreamOpt),
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

#[derive(StructOpt)]
struct StreamOpt {
    #[structopt(long, short)]
    pub group: Ipv4Addr,
    #[structopt(long, short)]
    pub port: u16,
    #[structopt(long, short)]
    pub bind: Option<SocketAddrV4>,
    #[structopt(long, default_value="20")]
    pub delay_ms: u64,
}

#[derive(Debug)]
enum RunError {
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
        Opt::Stream(opt) => run_stream(opt),
        Opt::Receive(opt) => run_receive(opt),
    };

    result.map_err(|err| {
        eprintln!("error: {err:?}");
        ExitCode::FAILURE
    })
}

fn run_stream(opt: StreamOpt) -> Result<(), RunError> {
    let host = cpal::default_host();

    let device = host.default_input_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = config_for_device(&device)?;

    let bind = opt.bind.unwrap_or(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, opt.port));

    let multicast_addr = SocketAddrV4::new(opt.group, opt.port);

    let socket = UdpSocket::bind(bind)
        .map_err(|e| RunError::BindSocket(bind, e))?;

    socket.join_multicast_v4(&opt.group, bind.ip())
        .map_err(RunError::JoinMulticast)?;

    // we don't need it:
    let _ = socket.set_multicast_loop_v4(false);

    let socket = Arc::new(socket);

    let delay = Duration::from_millis(opt.delay_ms);
    let delay = SampleDuration::from_std_duration_lossy(delay);

    let sid = TimestampMicros::now();

    let mut packet = AudioPacket {
        magic: protocol::MAGIC_AUDIO,
        flags: 0,
        sid,
        seq: 1,
        pts: TimestampMicros(0),
        buffer: PacketBuffer::zeroed(),
    };

    let mut packet_written = SampleDuration::zero();

    let stream = device.build_input_stream(&config,
        {
            let socket = Arc::clone(&socket);
            move |mut data: &[f32], _: &InputCallbackInfo| {
                // assert data only contains complete frames:
                assert!(data.len() % usize::from(protocol::CHANNELS) == 0);

                let mut timestamp = Timestamp::now().add(delay);

                if packet.pts.0 == 0 {
                    packet.pts = timestamp.to_micros_lossy();
                }

                while data.len() > 0 {
                    let buffer_offset = packet_written.as_buffer_offset();
                    let buffer_remaining = packet.buffer.0.len() - buffer_offset;

                    let copy_count = std::cmp::min(data.len(), buffer_remaining);
                    let buffer_copy_end = buffer_offset + copy_count;

                    packet.buffer.0[buffer_offset..buffer_copy_end]
                        .copy_from_slice(&data[0..copy_count]);

                    data = &data[copy_count..];
                    packet_written = SampleDuration::from_buffer_offset(buffer_copy_end);
                    timestamp = timestamp.add(SampleDuration::from_buffer_offset(copy_count));

                    if packet_written == SampleDuration::ONE_PACKET {
                        // packet is full! send:
                        socket.send_to(bytemuck::bytes_of(&packet), multicast_addr)
                            .expect("UdpSocket::send");

                        // reset rest of packet for next:
                        packet.seq += 1;
                        packet.pts = timestamp.to_micros_lossy();
                        packet_written = SampleDuration::zero();
                    }
                }

                // if there is data waiting in the packet buffer at the end of the
                // callback, the pts we just calculated is valid. if the packet is
                // empty, reset the pts to 0. this signals the next callback to set
                // pts to the current time when it fires.
                if packet_written == SampleDuration::zero() {
                    packet.pts.0 = 0;
                }
            }
        },
        move |err| {
            eprintln!("stream error! {err:?}");
        },
        None
    ).map_err(RunError::BuildStream)?;

    // set up t1 sender thread
    std::thread::spawn({
        let socket = Arc::clone(&socket);
        move || {
            loop {
                let t1 = TimestampMicros::now();

                let packet = TimePacket {
                    magic: protocol::MAGIC_TIME,
                    flags: 0,
                    sid,
                    t1,
                    t2: TimestampMicros(0),
                    t3: TimestampMicros(0),
                };

                socket.send_to(bytemuck::bytes_of(&packet), multicast_addr)
                    .expect("socket.send in time beat thread");

                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    stream.play().map_err(RunError::Stream)?;

    loop {
        let mut packet_raw = [0u8; MAX_PACKET_SIZE];

        let (nbytes, addr) = socket.recv_from(&mut packet_raw)
            .expect("socket.recv_from");

        match Packet::try_from_bytes_mut(&mut packet_raw[0..nbytes]) {
            Some(Packet::Audio(packet)) => {
                // we should only ever receive an audio packet if another
                // stream is present. check if it should take over
                if packet.sid.0 > sid.0 {
                    eprintln!("Another stream has taken over from {addr}, exiting");
                    break;
                }
            }
            Some(Packet::Time(packet)) => {
                // only handle packet if it belongs to our stream:
                if packet.sid.0 == sid.0 {
                    packet.t3 = TimestampMicros::now();
                    socket.send_to(bytemuck::bytes_of(packet), addr)
                        .expect("socket.send responding to time packet");
                }
            }
            None => {
                // unknown packet, ignore
            }
        }
    }

    Ok(())
}

fn run_receive(opt: ReceiveOpt) -> Result<(), RunError> {
    let host = cpal::default_host();

    let device = host.default_output_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = config_for_device(&device)?;

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

    loop {
        let mut packet_raw = [0u8; protocol::MAX_PACKET_SIZE];

        let (nbytes, addr) = socket.recv_from(&mut packet_raw)
            .map_err(RunError::Socket)?;

        match Packet::try_from_bytes_mut(&mut packet_raw[0..nbytes]) {
            Some(Packet::Time(packet)) => {
                if packet.t3.0 == 0 {
                    // we need to respond to this packet
                    packet.t2 = TimestampMicros::now();
                    socket.send_to(bytemuck::bytes_of(packet), addr)
                        .expect("reply to time packet");
                    continue;
                }

                let mut state = state.lock().unwrap();
                state.recv.receive_time(packet);
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

fn config_for_device(device: &cpal::Device) -> Result<StreamConfig, RunError> {
    let configs = device.supported_input_configs()
        .map_err(RunError::StreamConfigs)?;

    let config = configs
        .filter(|config| config.sample_format() == protocol::SAMPLE_FORMAT)
        .filter(|config| config.channels() == protocol::CHANNELS)
        .nth(0)
        .ok_or(RunError::NoSupportedStreamConfig)?;

    let buffer_size = match config.buffer_size() {
        SupportedBufferSize::Range { min, .. } => {
            std::cmp::max(*min, protocol::FRAMES_PER_PACKET as u32)
        }
        SupportedBufferSize::Unknown => {
            protocol::FRAMES_PER_PACKET as u32
        }
    };

    Ok(StreamConfig {
        channels: protocol::CHANNELS,
        sample_rate: protocol::SAMPLE_RATE,
        buffer_size: BufferSize::Fixed(buffer_size),
    })
}
