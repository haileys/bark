pub mod node;
pub mod receiver;
pub mod render;

use std::collections::HashMap;
use std::mem::size_of;
use std::net::{SocketAddrV4, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::io::Write;

use bytemuck::Zeroable;
use structopt::StructOpt;
use termcolor::BufferedStandardStream;

use crate::protocol::{StatsRequestPacket, self, StatsReplyPacket, StatsReplyFlags};
use crate::socket::{MultiSocket, SocketOpt};
use crate::RunError;

use self::render::Padding;

#[derive(StructOpt)]
pub struct StatsOpt {
    #[structopt(flatten)]
    pub socket: SocketOpt,
}

pub fn run(opt: StatsOpt) -> Result<(), RunError> {
    let socket = MultiSocket::open(opt.socket)
        .map_err(RunError::Listen)?;

    let socket = Arc::new(socket);

    // spawn poller thread
    std::thread::spawn({
        let socket = socket.clone();
        move || {
            loop {
                let packet = StatsRequestPacket {
                    magic: protocol::MAGIC_STATS_REQ,
                };

                let _ = socket.broadcast(bytemuck::bytes_of(&packet));

                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });

    let mut stats = HashMap::<SocketAddrV4, Entry>::new();

    loop {
        let mut reply = StatsReplyPacket::zeroed();
        let (nbytes, addr) = socket.recv_from(bytemuck::bytes_of_mut(&mut reply))
            .map_err(RunError::Socket)?;

        if nbytes != size_of::<StatsReplyPacket>() {
            continue;
        }

        if reply.magic != protocol::MAGIC_STATS_REPLY {
            continue;
        }

        let SocketAddr::V4(addr) = addr else {
            continue;
        };

        let prev_entries = stats.len();

        let now = Instant::now();
        stats.insert(addr, Entry { time: now, packet: Box::new(reply) });
        stats.retain(|_, ent| ent.valid_at(now));

        let mut out = BufferedStandardStream::stdout(termcolor::ColorChoice::Auto);

        // move cursor up:
        if prev_entries > 0 {
            let _ = write!(out, "\x1b[{prev_entries}F");
        }

        // write stats for stream sources first
        let mut stats = stats.iter().collect::<Vec<_>>();
        stats.sort_by_key(|(addr, entry)| (entry.is_receiver(), *addr));

        let mut padding = Padding::default();

        for (addr, entry) in &stats {
            render::calculate(&mut padding, &entry.packet, **addr);
        }

        for (addr, entry) in &stats {
            // kill line
            let _ = write!(out, "\x1b[2K");
            render::line(&mut out, &padding, &entry.packet, **addr);
            let _ = write!(out, "\n");
        }

        let _ = out.flush();
    }
}



struct Entry {
    time: Instant,
    packet: Box<StatsReplyPacket>,
}

impl Entry {
    pub fn is_receiver(&self) -> bool {
        self.packet.flags.contains(StatsReplyFlags::IS_RECEIVER)
    }

    pub fn valid_at(&self, now: Instant) -> bool {
        let age = now.duration_since(self.time);
        age < Duration::from_millis(1000)
    }
}
