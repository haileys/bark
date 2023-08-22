// use crate::socket::Socket;
// use crate::protocol;

// pub struct SourceProtocol {
//     socket: Socket,
// }

// impl SourceProtocol {
//     pub fn new(socket: Socket) -> Self {
//         SourceProtocol { socket }
//     }
// }

// pub struct AudioPacket {
//     raw: Box<protocol::types::AudioPacket>,
// }

// impl AudioPacket {
//     pub fn new() -> AudioPacket {
//         AudioPacket {
//             raw: bytemuck::allocation::zeroed_box(),
//         }
//     }
// }
