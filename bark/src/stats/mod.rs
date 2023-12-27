pub mod node;
pub mod render;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::io::Write;

use structopt::StructOpt;
use termcolor::BufferedStandardStream;

use bark_protocol::packet::{StatsRequest, StatsReply, PacketKind};
use bark_protocol::types::StatsReplyFlags;

use crate::socket::{Socket, SocketOpt, PeerId, ProtocolSocket};
use crate::RunError;

use self::render::Padding;

#[derive(StructOpt)]
pub struct StatsOpt {
    #[structopt(flatten)]
    pub socket: SocketOpt,
}

pub fn run(opt: StatsOpt) -> Result<(), RunError> {
    let socket = Socket::open(opt.socket)
        .map_err(RunError::Listen)?;

    let protocol = Arc::new(ProtocolSocket::new(socket));

    // spawn poller thread
    std::thread::spawn({
        let protocol = Arc::clone(&protocol);
        move || {
            let request = StatsRequest::new()
                .expect("allocate StatsRequest packet");

            loop {
                let _ = protocol.broadcast(request.as_packet());
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });

    let mut stats = HashMap::<PeerId, Entry>::new();

    loop {
        let (reply, peer) = protocol.recv_from().map_err(RunError::Receive)?;

        let Some(PacketKind::StatsReply(reply)) = reply.parse() else {
            continue;
        };

        let prev_entries = stats.len();

        let now = Instant::now();
        stats.insert(peer, Entry { time: now, reply });
        stats.retain(|_, ent| ent.valid_at(now));

        let current_entries = stats.len();

        let mut out = BufferedStandardStream::stdout(termcolor::ColorChoice::Auto);

        // move cursor up:
        move_cursor_up(&mut out, prev_entries);

        // write stats for stream sources first
        let mut stats = stats.iter().collect::<Vec<_>>();
        stats.sort_by_key(|(peer, entry)| (entry.is_receiver(), *peer));

        let mut padding = Padding::default();

        for (peer, entry) in &stats {
            render::calculate(&mut padding, entry.reply.data(), **peer);
        }

        for (peer, entry) in &stats {
            // kill line
            kill_line(&mut out);
            render::line(&mut out, &padding, &entry.reply, **peer);
            new_line(&mut out);
        }

        if current_entries < prev_entries {
            let remove_lines = prev_entries - current_entries;
            for _ in 0..remove_lines {
                kill_line(&mut out);
                new_line(&mut out);
            }
            move_cursor_up(&mut out, remove_lines);
        }

        let _ = out.flush();
    }
}

fn move_cursor_up(out: &mut BufferedStandardStream, lines: usize) {
    if lines > 0 {
        let _ = write!(out, "\x1b[{lines}F");
    }
}

fn kill_line(out: &mut BufferedStandardStream) {
    let _ = write!(out, "\x1b[2K\r");
}

fn new_line(out: &mut BufferedStandardStream) {
    let _ = write!(out, "\n");
}

struct Entry {
    time: Instant,
    reply: StatsReply,
}

impl Entry {
    pub fn is_receiver(&self) -> bool {
        self.reply.flags().contains(StatsReplyFlags::IS_RECEIVER)
    }

    pub fn valid_at(&self, now: Instant) -> bool {
        let age = now.duration_since(self.time);
        age < Duration::from_millis(1000)
    }
}
