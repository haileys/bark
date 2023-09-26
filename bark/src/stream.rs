use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{HostTrait, DeviceTrait, StreamTrait};
use cpal::InputCallbackInfo;
use structopt::StructOpt;

use bark_network::{Socket, ProtocolSocket};
use bark_protocol::time::{SampleDuration, Timestamp};
use bark_protocol::packet::{self, Audio, StatsReply, PacketKind};
use bark_protocol::types::{TimestampMicros, AudioPacketHeader, SessionId, ReceiverId, TimePhase};

use crate::{stats, time};
use crate::{RunError, SocketOpt};

#[derive(StructOpt)]
pub struct StreamOpt {
    #[structopt(flatten)]
    pub socket: SocketOpt,

    #[structopt(
        long,
        env = "BARK_SOURCE_DEVICE",
    )]
    pub device: Option<String>,

    #[structopt(
        long,
        env = "BARK_SOURCE_DELAY_MS",
        default_value = "20",
    )]
    pub delay_ms: u64,
}

pub fn run(opt: StreamOpt) -> Result<(), RunError> {
    let host = cpal::default_host();

    if let Some(device) = &opt.device {
        bark_device::env::set_source(device);
    }

    let device = host.default_input_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = bark_device::util::config_for_device(&device)
        .map_err(RunError::ConfigureDevice)?;

    let socket = Socket::open(opt.socket.multicast)
        .map_err(RunError::Listen)?;

    let protocol = Arc::new(ProtocolSocket::new(socket));

    let delay = Duration::from_millis(opt.delay_ms);
    let delay = SampleDuration::from_std_duration_lossy(delay);

    let sid = generate_session_id();
    let node = stats::node::get();

    let mut audio_header = AudioPacketHeader {
        sid,
        seq: 1,
        pts: TimestampMicros(0),
        dts: TimestampMicros(0),
    };

    let mut audio_buffer = Audio::write()
        .expect("allocate Audio packet");

    let stream = device.build_input_stream(&config,
        {
            let protocol = Arc::clone(&protocol);
            let mut initialized_thread = false;
            move |mut data: &[f32], _: &InputCallbackInfo| {
                if !initialized_thread {
                    crate::thread::set_name("bark/audio");
                    crate::thread::set_realtime_priority();
                    initialized_thread = true;
                }

                // assert data only contains complete frames:
                assert!(data.len() % usize::from(bark_protocol::CHANNELS) == 0);

                let mut timestamp = Timestamp::from_micros_lossy(time::now()).add(delay);

                if audio_header.pts.0 == 0 {
                    audio_header.pts = timestamp.to_micros_lossy();
                }

                while data.len() > 0 {
                    // write some data to the waiting packet buffer
                    let written = audio_buffer.write(data);

                    // advance
                    timestamp = timestamp.add(written);
                    data = &data[written.as_buffer_offset()..];

                    // if packet buffer is full, finalize it and send off the packet:
                    if audio_buffer.valid_length() {
                        // take packet writer and replace with new
                        let audio = std::mem::replace(&mut audio_buffer,
                            Audio::write().expect("allocate Audio packet"));

                        // finalize packet
                        let audio_packet = audio.finalize(AudioPacketHeader {
                            dts: time::now(),
                            ..audio_header
                        });

                        // send it
                        protocol.broadcast(audio_packet.as_packet()).expect("broadcast");

                        // reset header for next packet:
                        audio_header.seq += 1;
                        audio_header.pts = timestamp.to_micros_lossy();
                    }
                }

                // if there is data waiting in the packet buffer at the end of the
                // callback, the pts we just calculated is valid. if the packet is
                // empty, reset the pts to 0. this signals the next callback to set
                // pts to the current time when it fires.
                if audio_buffer.length() == SampleDuration::zero() {
                    audio_header.pts.0 = 0;
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
        crate::thread::set_name("bark/clock");
        crate::thread::set_realtime_priority();

        let protocol = Arc::clone(&protocol);
        move || {
            let mut time = packet::Time::allocate()
                .expect("allocate Time packet");

            // set up packet
            let data = time.data_mut();
            data.sid = sid;
            data.rid = ReceiverId::broadcast();

            loop {
                time.data_mut().stream_1 = time::now();

                protocol.broadcast(time.as_packet())
                    .expect("broadcast time");

                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    stream.play().map_err(RunError::Stream)?;

    crate::thread::set_name("bark/network");
    crate::thread::set_realtime_priority();

    loop {
        let (packet, peer) = protocol.recv_from().expect("protocol.recv_from");

        match packet.parse() {
            Some(PacketKind::Audio(audio)) => {
                // we should only ever receive an audio packet if another
                // stream is present. check if it should take over
                if audio.header().sid > sid {
                    eprintln!("Peer {peer} has taken over stream, exiting");
                    break;
                }
            }
            Some(PacketKind::Time(mut time)) => {
                // only handle packet if it belongs to our stream:
                if time.data().sid != sid {
                    continue;
                }

                match time.data().phase() {
                    Some(TimePhase::ReceiverReply) => {
                        time.data_mut().stream_3 = time::now();

                        protocol.send_to(time.as_packet(), peer)
                            .expect("protocol.send_to responding to time packet");
                    }
                    _ => {
                        // any other packet here must be destined for
                        // another instance on the same machine
                    }
                }

            }
            Some(PacketKind::StatsRequest(_)) => {
                let reply = StatsReply::source(sid, node)
                    .expect("allocate StatsReply packet");

                let _ = protocol.send_to(reply.as_packet(), peer);
            }
            Some(PacketKind::StatsReply(_)) => {
                // ignore
            }
            None => {
                // unknown packet, ignore
            }
        }
    }

    Ok(())
}

pub fn generate_session_id() -> SessionId {
    use nix::sys::time::TimeValLike;

    let timespec = nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME)
        .expect("clock_gettime(CLOCK_REALTIME)");

    SessionId(timespec.num_microseconds())
}
