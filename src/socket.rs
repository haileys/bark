use std::io;
use std::net::{Ipv4Addr, UdpSocket, SocketAddr, SocketAddrV4};
use std::os::fd::AsRawFd;

use nix::poll::{PollFd, PollFlags};
use socket2::{Socket, Domain, Type};
use structopt::StructOpt;

// expedited forwarding - IP header field indicating that switches should
// prioritise our packets for minimal delay
const IPTOS_DSCP_EF: u32 = 0xb8;

#[derive(Debug)]
pub enum ListenError {
    Socket(io::Error),
    SetReuseAddr(io::Error),
    SetBroadcast(io::Error),
    Bind(SocketAddrV4, io::Error),
    JoinMulticastGroup(Ipv4Addr, io::Error),
}

#[derive(StructOpt, Debug, Clone)]
pub struct SocketOpt {
    #[structopt(long, name="addr", env = "BARK_MULTICAST")]
    /// Multicast group address including port, eg. 224.100.100.100:1530
    pub multicast: SocketAddrV4,
}

pub struct MultiSocket {
    multicast: SocketAddrV4,

    // used to send unicast + multicast packets, as well as receive unicast replies
    // bound to 0.0.0.0:0, aka. OS picks a port
    tx: UdpSocket,

    // uses to receive multicast packets
    rx: UdpSocket,
}

impl MultiSocket {
    pub fn open(opt: SocketOpt) -> Result<MultiSocket, ListenError> {
        let group = *opt.multicast.ip();
        let port = opt.multicast.port();

        let tx = open_multicast(group, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        let rx = open_multicast(group, SocketAddrV4::new(group, port))?;

        Ok(MultiSocket {
            multicast: SocketAddrV4::new(group, port),
            tx: tx.into(),
            rx: rx.into(),
        })
    }

    pub fn broadcast(&self, msg: &[u8]) -> Result<(), io::Error> {
        self.send_to(msg, self.multicast.into())
    }

    pub fn send_to(&self, msg: &[u8], dest: SocketAddr) -> Result<(), io::Error> {
        self.tx.send_to(msg, dest)?;
        Ok(())
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr), io::Error> {
        let mut poll = [
            PollFd::new(self.tx.as_raw_fd(), PollFlags::POLLIN),
            PollFd::new(self.rx.as_raw_fd(), PollFlags::POLLIN),
        ];

        nix::poll::poll(&mut poll, -1)?;

        if poll[0].any() == Some(true) {
            self.tx.recv_from(buf)
        } else if poll[1].any() == Some(true) {
            self.rx.recv_from(buf)
        } else {
            unreachable!()
        }
    }
}

fn open_multicast(group: Ipv4Addr, bind: SocketAddrV4) -> Result<Socket, ListenError> {
    let socket = bind_socket(bind)?;

    // join multicast group
    socket.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| ListenError::JoinMulticastGroup(group, e))?;

    // set opts
    socket.set_broadcast(true).map_err(ListenError::SetBroadcast)?;
    let _ = socket.set_multicast_loop_v4(true);

    Ok(socket.into())
}

fn bind_socket(bind: SocketAddrV4) -> Result<Socket, ListenError> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)
        .map_err(ListenError::Socket)?;

    socket.set_reuse_address(true).map_err(ListenError::SetReuseAddr)?;

    if let Err(e) = socket.set_tos(IPTOS_DSCP_EF) {
        eprintln!("warning: failed to set IPTOS_DSCP_EF: {e:?}");
    }

    socket.bind(&bind.into()).map_err(|e| ListenError::Bind(bind, e))?;

    Ok(socket)
}
