use std::sync::Arc;
use std::time::Duration;

use bark_protocol::SAMPLES_PER_PACKET;
use structopt::StructOpt;

use bark_protocol::time::SampleDuration;
use bark_protocol::packet::{self, Audio, StatsReply, PacketKind};
use bark_protocol::types::{TimestampMicros, AudioPacketHeader, SessionId, ReceiverId, TimePhase, AudioPacketFormat};

use crate::audio::config::{DeviceOpt, DEFAULT_PERIOD, DEFAULT_BUFFER};
use crate::audio::input::Input;
use crate::socket::{Socket, SocketOpt, ProtocolSocket};
use crate::{stats, time};
use crate::RunError;

#[derive(StructOpt)]
pub struct StreamOpt {
    #[structopt(flatten)]
    pub socket: SocketOpt,

    /// Audio device name
    #[structopt(long, env = "BARK_SOURCE_INPUT_DEVICE")]
    pub input_device: Option<String>,

    /// Size of discrete audio transfer buffer in frames
    #[structopt(long, env = "BARK_SOURCE_INPUT_PERIOD")]
    pub input_period: Option<u64>,

    /// Size of decoded audio buffer in frames
    #[structopt(long, env = "BARK_SOURCE_INPUT_BUFFER")]
    pub input_buffer: Option<u64>,

    #[structopt(
        long,
        env = "BARK_SOURCE_DELAY_MS",
        default_value = "20",
    )]
    pub delay_ms: u64,
}

pub fn run(opt: StreamOpt) -> Result<(), RunError> {
    let input = Input::new(DeviceOpt {
        device: opt.input_device,
        period: opt.input_period
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_PERIOD),
        buffer: opt.input_buffer
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_BUFFER),
    }).map_err(RunError::OpenAudioDevice)?;

    let socket = Socket::open(opt.socket)
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
        format: AudioPacketFormat::F32LE,
    };

    std::thread::spawn({
        let protocol = protocol.clone();
        move || {
            crate::thread::set_name("bark/audio");

            loop {
                // create new audio buffer
                let buffer_bytes_length = core::mem::size_of::<f32>() * SAMPLES_PER_PACKET;
                let mut audio = Audio::allocate(buffer_bytes_length)
                    .expect("allocate Audio packet");

                let sample_buffer = bytemuck::cast_slice_mut(audio.buffer_bytes_mut());

                // read audio input
                let timestamp = match input.read(sample_buffer) {
                    Ok(ts) => ts,
                    Err(e) => {
                        log::error!("error reading audio input: {e}");
                        break;
                    }
                };

                let pts = timestamp.add(delay);

                // write packet header
                *audio.header_mut() = AudioPacketHeader {
                    pts: pts.to_micros_lossy(),
                    dts: time::now(),
                    ..audio_header
                };

                // send it
                protocol.broadcast(audio.as_packet()).expect("broadcast");

                // reset header for next packet:
                audio_header.seq += 1;
            }
        }
    });

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

    crate::thread::set_name("bark/network");
    crate::thread::set_realtime_priority();

    loop {
        let (packet, peer) = protocol.recv_from().expect("protocol.recv_from");

        match packet.parse() {
            Some(PacketKind::Audio(audio)) => {
                // we should only ever receive an audio packet if another
                // stream is present. check if it should take over
                if audio.header().sid > sid {
                    log::warn!("peer {peer} has taken over stream, exiting");
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
