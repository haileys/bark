use std::net::{UdpSocket, Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use bytemuck::Zeroable;
use cpal::traits::{HostTrait, DeviceTrait, StreamTrait};
use cpal::InputCallbackInfo;
use structopt::StructOpt;

use crate::protocol::{self, Packet, TimestampMicros, AudioPacket, PacketBuffer, TimePacket, MAX_PACKET_SIZE, TimePacketPadding, SessionId};
use crate::time::{SampleDuration, Timestamp};
use crate::util;
use crate::RunError;

#[derive(StructOpt)]
pub struct StreamOpt {
    #[structopt(long, short)]
    pub group: Ipv4Addr,
    #[structopt(long, short)]
    pub port: u16,
    #[structopt(long, short)]
    pub bind: Option<SocketAddrV4>,
    #[structopt(long, default_value="20")]
    pub delay_ms: u64,
}

pub fn run(opt: StreamOpt) -> Result<(), RunError> {
    let host = cpal::default_host();

    let device = host.default_input_device()
        .ok_or(RunError::NoDeviceAvailable)?;

    let config = util::config_for_device(&device)?;

    let bind = opt.bind.unwrap_or(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, opt.port));

    let multicast_addr = SocketAddrV4::new(opt.group, opt.port);

    let socket = UdpSocket::bind(bind)
        .map_err(|e| RunError::BindSocket(bind, e))?;

    socket.join_multicast_v4(&opt.group, bind.ip())
        .map_err(RunError::JoinMulticast)?;

    util::set_expedited_forwarding(&socket);

    // we don't need it:
    let _ = socket.set_multicast_loop_v4(false);

    let socket = Arc::new(socket);

    let delay = Duration::from_millis(opt.delay_ms);
    let delay = SampleDuration::from_std_duration_lossy(delay);

    let sid = SessionId::generate();

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
                        // packet is full! set dts and send
                        packet.dts = TimestampMicros::now();
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
        // this thread broadcasts time packets
        util::set_realtime_priority(99);

        let socket = Arc::clone(&socket);
        move || {
            loop {
                let now = TimestampMicros::now();

                let packet = TimePacket {
                    magic: protocol::MAGIC_TIME,
                    flags: 0,
                    sid,
                    stream_1: now,
                    receive_2: TimestampMicros(0),
                    stream_3: TimestampMicros(0),
                    _pad: TimePacketPadding::zeroed(),
                };

                socket.send_to(bytemuck::bytes_of(&packet), multicast_addr)
                    .expect("socket.send in time beat thread");

                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    stream.play().map_err(RunError::Stream)?;

    // this thread responds to time packets
    util::set_realtime_priority(99);

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
                if packet.sid == sid {
                    packet.stream_3 = TimestampMicros::now();
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
