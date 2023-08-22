use std::sync::Arc;
use std::time::Duration;

use bytemuck::Zeroable;
use cpal::traits::{HostTrait, DeviceTrait, StreamTrait};
use cpal::InputCallbackInfo;
use structopt::StructOpt;

use crate::protocol::{self, Packet, TimestampMicros, AudioPacket, PacketBuffer, TimePacket, MAX_PACKET_SIZE, TimePacketPadding, SessionId, ReceiverId, TimePhase, StatsReplyPacket, StatsReplyFlags};
use crate::socket::{Socket, SocketOpt};
use crate::stats::node::NodeStats;
use crate::stats::receiver::ReceiverStats;
use crate::time::{SampleDuration, Timestamp};
use crate::util;
use crate::RunError;

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
        crate::audio::set_source_env(device);
    }

    let device = host.default_input_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = util::config_for_device(&device)?;

    let socket = Socket::open(opt.socket)
        .map_err(RunError::Listen)?;

    let socket = Arc::new(socket);

    let delay = Duration::from_millis(opt.delay_ms);
    let delay = SampleDuration::from_std_duration_lossy(delay);

    let sid = SessionId::generate();
    let node = NodeStats::get();

    let mut packet = AudioPacket {
        magic: protocol::MAGIC_AUDIO,
        flags: 0,
        sid,
        seq: 1,
        pts: TimestampMicros(0),
        dts: TimestampMicros(0),
        buffer: PacketBuffer::zeroed(),
    };

    let mut packet_written = SampleDuration::zero();

    let stream = device.build_input_stream(&config,
        {
            let socket = Arc::clone(&socket);
            let mut initialized_thread = false;
            move |mut data: &[f32], _: &InputCallbackInfo| {
                if !initialized_thread {
                    crate::thread::set_name("bark/audio");
                    crate::thread::set_realtime_priority();
                    initialized_thread = true;
                }

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
                        // packet is full! set dts and send
                        packet.dts = TimestampMicros::now();
                        socket.broadcast(bytemuck::bytes_of(&packet)).expect("broadcast");

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
        crate::thread::set_name("bark/clock");
        crate::thread::set_realtime_priority();

        let socket = Arc::clone(&socket);
        move || {
            loop {
                let now = TimestampMicros::now();

                let packet = TimePacket {
                    magic: protocol::MAGIC_TIME,
                    flags: 0,
                    sid,
                    rid: ReceiverId::broadcast(),
                    stream_1: now,
                    receive_2: TimestampMicros(0),
                    stream_3: TimestampMicros(0),
                    _pad: TimePacketPadding::zeroed(),
                };

                socket.broadcast(bytemuck::bytes_of(&packet))
                    .expect("broadcast time");

                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    stream.play().map_err(RunError::Stream)?;

    crate::thread::set_name("bark/network");
    crate::thread::set_realtime_priority();

    loop {
        let mut packet_raw = [0u8; MAX_PACKET_SIZE];

        let (nbytes, addr) = socket.recv_from(&mut packet_raw)
            .expect("socket.recv_from");

        match Packet::try_from_bytes_mut(&mut packet_raw[0..nbytes]) {
            Some(Packet::Audio(packet)) => {
                // we should only ever receive an audio packet if another
                // stream is present. check if it should take over
                if packet.sid > sid {
                    eprintln!("Another stream has taken over from {addr}, exiting");
                    break;
                }
            }
            Some(Packet::Time(packet)) => {
                // only handle packet if it belongs to our stream:
                if packet.sid != sid {
                    continue;
                }

                match packet.phase() {
                    Some(TimePhase::ReceiverReply) => {
                        packet.stream_3 = TimestampMicros::now();

                        socket.send_to(bytemuck::bytes_of(packet), addr)
                            .expect("socket.send responding to time packet");
                    }
                    _ => {
                        // any other packet here must be destined for
                        // another instance on the same machine
                    }
                }

            }
            Some(Packet::StatsRequest(_)) => {
                let reply = StatsReplyPacket {
                    magic: protocol::MAGIC_STATS_REPLY,
                    flags: StatsReplyFlags::IS_STREAM,
                    sid: sid,
                    receiver: ReceiverStats::zeroed(),
                    node,
                };

                let _ = socket.send_to(bytemuck::bytes_of(&reply), addr);
            }
            Some(Packet::StatsReply(_)) => {
                // ignore
            }
            None => {
                // unknown packet, ignore
            }
        }
    }

    Ok(())
}
