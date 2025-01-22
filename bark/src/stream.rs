use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bark_core::audio::{Format, F32, S16};
use bark_core::encode::Encode;
use bark_core::encode::pcm::{S16LEEncoder, F32LEEncoder};
use bark_protocol::FRAMES_PER_PACKET;
use bytemuck::Zeroable;
use futures::future;
use structopt::StructOpt;

#[cfg(feature = "opus")]
use bark_core::encode::opus::OpusEncoder;

use bark_protocol::time::SampleDuration;
use bark_protocol::packet::{Audio, PacketKind, Pong, StatsReply};
use bark_protocol::types::{TimestampMicros, AudioPacketHeader, SessionId};

use crate::audio::config::{DeviceOpt, DEFAULT_PERIOD, DEFAULT_BUFFER};
use crate::audio::Input;
use crate::socket::{Socket, SocketOpt, ProtocolSocket};
use crate::stats::server::MetricsOpt;
use crate::stats::SourceMetrics;
use crate::{config, stats, thread, time};
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
    pub input_period: Option<usize>,

    /// Size of decoded audio buffer in frames
    #[structopt(long, env = "BARK_SOURCE_INPUT_BUFFER")]
    pub input_buffer: Option<usize>,

    #[structopt(long, env = "BARK_SOURCE_INPUT_FORMAT", default_value = "f32")]
    pub input_format: config::Format,

    #[structopt(
        long,
        env = "BARK_SOURCE_DELAY_MS",
        default_value = "20",
    )]
    pub delay_ms: u64,

    #[structopt(
        long,
        env = "BARK_SOURCE_CODEC",
        default_value = "f32le",
    )]
    pub format: config::Codec,
}

pub async fn run(opt: StreamOpt, metrics: MetricsOpt) -> Result<(), RunError> {
    let socket = Socket::open(&opt.socket)?;
    let protocol = Arc::new(ProtocolSocket::new(socket));

    let sid = generate_session_id();

    let metrics = stats::server::start_source(&metrics).await?;

    let audio_th = match opt.input_format {
        config::Format::S16 => start_audio_thread::<S16>(opt, protocol.clone(), sid, metrics)?,
        config::Format::F32 => start_audio_thread::<F32>(opt, protocol.clone(), sid, metrics)?,
    };

    let network_th = thread::start("bark/network", {
        move || network_thread(sid, protocol)
    });

    future::select(audio_th, network_th).await;
    Ok(())
}

fn start_audio_thread<F: Format>(
    opt: StreamOpt,
    protocol: Arc<ProtocolSocket>,
    sid: SessionId,
    _metrics: SourceMetrics,
) -> Result<Pin<Box<dyn Future<Output = ()>>>, RunError> {
    let input = Input::<F>::new(&DeviceOpt {
        device: opt.input_device,
        period: opt.input_period
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_PERIOD),
        buffer: opt.input_buffer
            .map(SampleDuration::from_frame_count)
            .unwrap_or(DEFAULT_BUFFER),
    })?;

    let encoder: Box<dyn Encode> = match opt.format {
        config::Codec::S16LE => Box::new(S16LEEncoder),
        config::Codec::F32LE => Box::new(F32LEEncoder),
        #[cfg(feature = "opus")]
        config::Codec::Opus => Box::new(OpusEncoder::new()?),
    };

    log::info!("instantiated encoder: {}", encoder);

    let delay = Duration::from_millis(opt.delay_ms);
    let delay = SampleDuration::from_std_duration_lossy(delay);

    let audio_th = thread::start("bark/audio", {
        let protocol = protocol.clone();
        move || audio_thread(input, encoder, delay, sid, protocol)
    });

    Ok(Box::pin(audio_th))
}

fn audio_thread<F: Format>(
    input: Input<F>,
    mut encoder: Box<dyn Encode>,
    delay: SampleDuration,
    sid: SessionId,
    protocol: Arc<ProtocolSocket>,
) {
    thread::set_realtime_priority();

    let mut audio_header = AudioPacketHeader {
        sid,
        seq: 1,
        pts: TimestampMicros(0),
        dts: TimestampMicros(0),
        format: encoder.header_format(),
        priority: 0,
        padding: Default::default(),
    };

    loop {
        let mut audio_buffer = [F::Frame::zeroed(); FRAMES_PER_PACKET];

        // read audio input
        let timestamp = match input.read(&mut audio_buffer) {
            Ok(ts) => ts,
            Err(e) => {
                log::error!("error reading audio input: {e}");
                break;
            }
        };

        // encode audio
        let mut encode_buffer = [0; Audio::MAX_BUFFER_LENGTH];
        let encoded_data = match encoder.encode_packet(F::frames(&audio_buffer), &mut encode_buffer) {
            Ok(size) => &encode_buffer[0..size],
            Err(e) => {
                log::error!("error encoding audio: {e}");
                break;
            }
        };

        // assemble new packet header
        let pts = timestamp.add(delay);

        let header = AudioPacketHeader {
            pts: pts.to_micros_lossy(),
            dts: time::now(),
            ..audio_header
        };

        // allocate new audio packet and copy encoded data in
        let audio = Audio::new(&header, encoded_data)
            .expect("allocate Audio packet");

        // send it
        protocol.broadcast(audio.as_packet()).expect("broadcast");

        // reset header for next packet:
        audio_header.seq += 1;
    }
}

fn network_thread(
    sid: SessionId,
    protocol: Arc<ProtocolSocket>,
) {
    thread::set_realtime_priority();
    let node = stats::node::get();

    loop {
        let (packet, peer) = protocol.recv_from().expect("protocol.recv_from");

        match packet.parse() {
            Some(PacketKind::Audio(_)) => {
                // ignore
            }
            Some(PacketKind::StatsRequest(_)) => {
                let reply = StatsReply::source(sid, node)
                    .expect("allocate StatsReply packet");

                let _ = protocol.send_to(reply.as_packet(), peer);
            }
            Some(PacketKind::StatsReply(_)) => {
                // ignore
            }
            Some(PacketKind::Ping(_)) => {
                let pong = Pong::new().expect("allocate Pong packet");
                let _ = protocol.send_to(pong.as_packet(), peer);
            }
            Some(PacketKind::Pong(_)) => {
                // ignore
            }
            None => {
                // unknown packet, ignore
            }
        }
    }
}

fn generate_session_id() -> SessionId {
    use nix::sys::time::TimeValLike;

    let timespec = nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME)
        .expect("clock_gettime(CLOCK_REALTIME)");

    SessionId(timespec.num_microseconds())
}
