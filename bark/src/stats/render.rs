use termcolor::{WriteColor, ColorSpec, Color};

use bark_protocol::packet::StatsReply;
use bark_protocol::types::{StatsReplyPacket, StatsReplyFlags};
use bark_protocol::types::stats::receiver::{ReceiverStats, StreamStatus};
use bark_protocol::types::stats::node::NodeStats;

use crate::socket::PeerId;
use super::node;

#[derive(Default)]
pub struct Padding {
    node_width: usize,
    peer_width: usize,
}

pub fn calculate(padding: &mut Padding, stats: &StatsReplyPacket, peer: PeerId) {
    let node_width = node::display(&stats.node).len();
    let peer_width = peer.to_string().len();

    padding.node_width = std::cmp::max(padding.node_width, node_width);
    padding.peer_width = std::cmp::max(padding.peer_width, peer_width);
}

pub fn line(out: &mut dyn WriteColor, padding: &Padding, stats: &StatsReply, peer: PeerId) {
    node(out, padding, &stats.data().node, peer);

    if stats.flags().contains(StatsReplyFlags::IS_RECEIVER) {
        receiver(out, &stats.data().receiver);
    } else if stats.flags().contains(StatsReplyFlags::IS_STREAM) {
        let _ = out.set_color(&ColorSpec::new()
            .set_fg(Some(Color::White))
            .set_bold(true));
        let _ = write!(out, "stream source");
        let _ = out.set_color(&ColorSpec::new());
    }
}

fn node(out: &mut dyn WriteColor, padding: &Padding, node: &NodeStats, peer: PeerId) {
    let _ = out.set_color(&ColorSpec::new()
        .set_fg(Some(Color::Blue))
        .set_bold(true));

    let _ = write!(out, "{:<width$}  ", node::display(node), width = padding.node_width);

    let _ = out.set_color(&ColorSpec::new()
        .set_dimmed(true));

    let _ = write!(out, "{:<width$}  ", peer, width = padding.peer_width);

    let _ = out.set_color(&ColorSpec::new());
}

fn receiver(out: &mut dyn WriteColor, stats: &ReceiverStats) {
    stream_status(out, stats.stream());

    time_field(out, "Audio", stats.audio_latency());
    time_field(out, "Buffer", stats.buffer_length());
    time_field(out, "Output", stats.output_latency());
    time_field(out, "Network", stats.network_latency());
    time_field(out, "Predict", stats.predict_offset());
}

fn stream_status(out: &mut dyn WriteColor, stream: Option<StreamStatus>) {
    let (color, label) = indicator_style(stream);
    let _ = out.set_color(&color);
    let _ = write!(out, "  {}  ", label);
    let _ = out.set_color(&ColorSpec::new());
}

fn indicator_style(value: Option<StreamStatus>) -> (ColorSpec, &'static str) {
    let mut spec = ColorSpec::new();
    let text;

    match value {
        Some(StreamStatus::Seek) => {
            text = "SEEK";
            spec.set_dimmed(true);
        }
        Some(StreamStatus::Sync) => {
            text = "SYNC";
            spec.set_bg(Some(Color::Green))
                .set_fg(Some(Color::Rgb(0, 0, 0))) // dark black
                .set_bold(true)
                .set_intense(true);
        }
        Some(StreamStatus::Slew) => {
            text = "SLEW";
            spec.set_bg(Some(Color::Yellow))
                .set_fg(Some(Color::Rgb(0, 0, 0))) // dark black
                .set_bold(true)
                .set_intense(true);
        }
        Some(StreamStatus::Miss) => {
            text = "MISS";
            spec.set_bg(Some(Color::Red))
                .set_fg(Some(Color::Rgb(0, 0, 0))) // dark black
                .set_bold(true)
                .set_intense(true);
        }
        None => {
            text = "    ";
        }
    }

    (spec, text)
}

fn time_field(out: &mut dyn WriteColor, name: &str, value: Option<f64>) {
    if let Some(secs) = value {
        let _ = write!(out, "  {name}:[{:>8.3} ms]", secs * 1000.0);
    } else {
        let _ = write!(out, "  {name}:[        ms]");
    }
}
