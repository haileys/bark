// pub mod source;
pub mod types;
pub mod packet;

use std::io;

pub use cpal::{SampleFormat, SampleRate, ChannelCount};

pub const SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;
pub const SAMPLE_RATE: SampleRate = SampleRate(48000);
pub const CHANNELS: ChannelCount = 2;
pub const FRAMES_PER_PACKET: usize = 160;
pub const SAMPLES_PER_PACKET: usize = CHANNELS as usize * FRAMES_PER_PACKET;

use crate::socket::{Socket, PeerId};
use crate::protocol::packet::PacketBuffer;

use self::packet::Packet;

pub struct Protocol {
    socket: Socket,
}

impl Protocol {
    pub fn new(socket: Socket) -> Self {
        Protocol { socket }
    }

    pub fn broadcast(&self, packet: &Packet) -> Result<(), io::Error> {
        self.socket.broadcast(packet.as_buffer().as_bytes())
    }

    pub fn send_to(&self, packet: &Packet, peer: PeerId) -> Result<(), io::Error> {
        self.socket.send_to(packet.as_buffer().as_bytes(), peer)
    }

    pub fn recv_from(&self) -> Result<(Packet, PeerId), io::Error> {
        loop {
            let mut buffer = PacketBuffer::allocate();

            let (nbytes, peer) = self.socket.recv_from(buffer.as_full_buffer_mut())?;
            buffer.set_len(nbytes);

            if let Some(packet) = Packet::from_buffer(buffer) {
                return Ok((packet, peer));
            }
        }
    }
}
