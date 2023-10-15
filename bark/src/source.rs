use std::sync::Arc;
use std::time::Duration;

use structopt::StructOpt;

use bark_network::{Socket, ProtocolSocket};
use bark_protocol::time::{SampleDuration, Timestamp};
use bark_protocol::packet::{self, StatsReply, PacketKind};
use bark_protocol::types::{SessionId, ReceiverId, TimePhase};

use crate::stats;
use crate::{RunError, SocketOpt};

use self::encode::Encode;

mod encode;

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
    if let Some(device) = &opt.device {
        bark_device::env::set_source(device);
    }

    let socket = Socket::open(opt.socket.multicast)
        .map_err(RunError::Listen)?;

    let protocol = Arc::new(ProtocolSocket::new(socket));

    let delay = Duration::from_millis(opt.delay_ms);
    let delay = SampleDuration::from_std_duration_lossy(delay);

    let sid = generate_session_id();
    let node = stats::node::get();

    // set up t1 sender thread
    std::thread::spawn({
        let protocol = Arc::clone(&protocol);
        move || {
            bark_util::thread::set_name("bark/clock");
            bark_util::thread::set_realtime_priority();

            let mut time = packet::Time::allocate()
                .expect("allocate Time packet");

            // set up packet
            let data = time.data_mut();
            data.sid = sid;
            data.rid = ReceiverId::broadcast();

            loop {
                time.data_mut().stream_1 = bark_util::time::now();

                protocol.broadcast(time.as_packet())
                    .expect("broadcast time");

                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    // start network thread
    std::thread::spawn({
        let protocol = Arc::clone(&protocol);
        move || {
            bark_util::thread::set_name("bark/network");
            bark_util::thread::set_realtime_priority();

            loop {
                let (packet, peer) = protocol.recv_from().expect("protocol.recv_from");

                match packet {
                    PacketKind::Audio(audio) => {
                        // we should only ever receive an audio packet if another
                        // stream is present. check if it should take over
                        if audio.header().sid > sid {
                            log::warn!("Peer {peer} has taken over stream, exiting");
                            break;
                        }
                    }
                    PacketKind::Time(mut time) => {
                        // only handle packet if it belongs to our stream:
                        if time.data().sid != sid {
                            continue;
                        }

                        match time.data().phase() {
                            Some(TimePhase::ReceiverReply) => {
                                time.data_mut().stream_3 = bark_util::time::now();

                                protocol.send_to(time.as_packet(), peer)
                                    .expect("protocol.send_to responding to time packet");
                            }
                            _ => {
                                // any other packet here must be destined for
                                // another instance on the same machine
                            }
                        }

                    }
                    PacketKind::StatsRequest(_) => {
                        let reply = StatsReply::source(sid, node)
                            .expect("allocate StatsReply packet");

                        let _ = protocol.send_to(reply.as_packet(), peer);
                    }
                    PacketKind::StatsReply(_) => {
                        // ignore
                    }
                }
            }
        }
    });

    // run encode on main thread
    let mut audio_source = bark_device::source::open()
        .map_err(RunError::OpenDevice)?;


    let codec = Box::new(encode::F32);
    let mut encode = Encode::new(protocol.clone(), codec, sid);

    while let Some(packet) = audio_source.read() {
        let timestamp = Timestamp::from_micros_lossy(packet.timestamp) + delay;
        encode.write(timestamp, &packet.data);
    }

    Ok(())
}

pub fn generate_session_id() -> SessionId {
    use nix::sys::time::TimeValLike;

    let timespec = nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME)
        .expect("clock_gettime(CLOCK_REALTIME)");

    SessionId(timespec.num_microseconds())
}
