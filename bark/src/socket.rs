use std::io;
use std::net::{Ipv4Addr, UdpSocket, SocketAddr, SocketAddrV4};
use std::os::fd::AsFd;

use derive_more::Display;
use nix::poll::{PollFd, PollFlags, PollTimeout};
use socket2::{Domain, Type};
use structopt::StructOpt;

use bark_protocol::buffer::PacketBuffer;
use bark_protocol::packet::Packet;
use thiserror::Error;

// expedited forwarding - IP header field indicating that switches should
// prioritise our packets for minimal delay
const IPTOS_DSCP_EF: u32 = 0xb8;

#[derive(Debug, Error)]
pub enum ListenError {
    #[error("creating socket: {0}")]
    Socket(io::Error),
    #[error("setting SO_REUSEADDR: {0}")]
    SetReuseAddr(io::Error),
    #[error("setting SO_BROADCAST: {0}")]
    SetBroadcast(io::Error),
    #[error("binding {0}: {1}")]
    Bind(SocketAddrV4, io::Error),
    #[error("joining multicast group {0}: {1}")]
    JoinMulticastGroup(Ipv4Addr, io::Error),
}

#[derive(StructOpt, Debug, Clone)]
pub struct SocketOpt {
    #[structopt(long, name="addr", env = "BARK_MULTICAST")]
    /// Multicast group address including port, eg. 224.100.100.100:1530
    pub multicast: SocketAddrV4,
}

pub struct Socket {
    multicast: SocketAddrV4,

    // used to send unicast + multicast packets, as well as receive unicast replies
    // bound to 0.0.0.0:0, aka. OS picks a port
    tx: UdpSocket,

    // uses to receive multicast packets
    rx: UdpSocket,
}

#[derive(Clone, Copy, Debug, Display, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerId(SocketAddr);

impl Socket {
    pub fn open(opt: SocketOpt) -> Result<Socket, ListenError> {
        let group = *opt.multicast.ip();
        let port = opt.multicast.port();

        let tx = open_multicast(group, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        let rx = open_multicast(group, SocketAddrV4::new(group, port))?;

        Ok(Socket {
            multicast: SocketAddrV4::new(group, port),
            tx: tx.into(),
            rx: rx.into(),
        })
    }

    pub fn broadcast(&self, msg: &[u8]) -> Result<(), io::Error> {
        self.tx.send_to(msg, self.multicast)?;
        Ok(())
    }

    pub fn send_to(&self, msg: &[u8], dest: PeerId) -> Result<(), io::Error> {
        self.tx.send_to(msg, dest.0)?;
        Ok(())
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, PeerId), io::Error> {
        let mut poll = [
            PollFd::new(self.tx.as_fd(), PollFlags::POLLIN),
            PollFd::new(self.rx.as_fd(), PollFlags::POLLIN),
        ];

        nix::poll::poll(&mut poll, PollTimeout::NONE)?;

        let (nbytes, addr) =
            if poll[0].any() == Some(true) {
                self.tx.recv_from(buf)?
            } else if poll[1].any() == Some(true) {
                self.rx.recv_from(buf)?
            } else {
                unreachable!("poll returned with no readable sockets");
            };

        Ok((nbytes, PeerId(addr)))
    }
}

fn open_multicast(group: Ipv4Addr, bind: SocketAddrV4) -> Result<socket2::Socket, ListenError> {
    let socket = bind_socket(bind)?;

    // join multicast group
    socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| ListenError::JoinMulticastGroup(group, e))?;

    // set opts
    socket.set_broadcast(true).map_err(ListenError::SetBroadcast)?;
    let _ = socket.set_multicast_loop_v4(true);

    Ok(socket.into())
}

fn bind_socket(bind: SocketAddrV4) -> Result<socket2::Socket, ListenError> {
    let socket = socket2::Socket::new(Domain::IPV4, Type::DGRAM, None)
        .map_err(ListenError::Socket)?;

    socket.set_reuse_address(true).map_err(ListenError::SetReuseAddr)?;

    if let Err(e) = socket.set_tos(IPTOS_DSCP_EF) {
        log::warn!("failed to set IPTOS_DSCP_EF: {e:?}");
    }

    socket.bind(&bind.into()).map_err(|e| ListenError::Bind(bind, e))?;

    Ok(socket)
}

pub struct ProtocolSocket {
    socket: Socket,
}

impl ProtocolSocket {
    pub fn new(socket: Socket) -> Self {
        ProtocolSocket { socket }
    }

    pub fn broadcast(&self, packet: &Packet) -> Result<(), io::Error> {
        self.socket.broadcast(packet.as_buffer().as_bytes())
    }

    pub fn send_to(&self, packet: &Packet, peer: PeerId) -> Result<(), io::Error> {
        self.socket.send_to(packet.as_buffer().as_bytes(), peer)
    }

    fn recv_buffer_from(&self) -> Result<(PacketBuffer, PeerId), io::Error> {
        let mut buffer = vec![0u8; bark_protocol::packet::MAX_PACKET_SIZE];

        let (nbytes, peer) = self.socket.recv_from(&mut buffer)?;

        // shrink vec to what we just read:
        assert!(nbytes <= buffer.len());
        buffer.resize(nbytes, 0);

        let buffer = PacketBuffer::from_raw(buffer);

        Ok((buffer, peer))
    }

    pub fn recv_from(&self) -> Result<(Packet, PeerId), io::Error> {
        loop {
            let (buffer, peer) = self.recv_buffer_from()?;

            if let Some(packet) = Packet::from_buffer(buffer) {
                return Ok((packet, peer));
            }
        }
    }
}
