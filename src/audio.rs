use std::process::{Command, Stdio};

use serde::Deserialize;

pub fn set_sink_env(device: &str) {
    let Some(index) = find_pulse_node(Kind::Sink, device) else {
        eprintln!("falling back to default audio sink");
        return;
    };

    println!("using audio sink at index {}: {}", index.0, device);

    std::env::set_var("PULSE_SINK", device);
    std::env::set_var("PIPEWIRE_NODE", index.0.to_string());
}

pub fn set_source_env(device: &str) {
    let Some(index) = find_pulse_node(Kind::Source, device) else {
        eprintln!("falling back to default audio source");
        return;
    };

    println!("using audio source at index {}: {}", index.0, device);

    std::env::set_var("PULSE_SOURCE", device);
    std::env::set_var("PIPEWIRE_NODE", index.0.to_string());
}

enum Kind {
    Source,
    Sink,
}

#[derive(Deserialize)]
struct Node {
    index: NodeIndex,
    name: String,
}

#[derive(Deserialize)]
struct NodeIndex(u64);

fn find_pulse_node(kind: Kind, name: &str) -> Option<NodeIndex> {
    let kind = match kind {
        Kind::Source => "sources",
        Kind::Sink => "sinks",
    };

    let result = Command::new("pactl")
        .args(["--format=json", "list", kind])
        .stdout(Stdio::piped())
        .output();

    let output = match result {
        Ok(output) => output,
        Err(e) => {
            eprintln!("error running pactl to find audio device: {e:?}");
            return None;
        }
    };

    let text = match std::str::from_utf8(&output.stdout) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("could not parse pactl output: {e:?}");
            return None;
        }
    };

    let nodes = match serde_json::from_str::<Vec<Node>>(text) {
        Ok(nodes) => nodes,
        Err(e) => {
            eprintln!("could not parse pactl output: {e:?}");
            return None;
        }
    };

    nodes.into_iter()
        .find(|node| node.name == name)
        .map(|node| node.index)
}
