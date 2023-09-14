use bytemuck::{Zeroable, Pod};

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct NodeStats {
    pub username: [u8; 32],
    pub hostname: [u8; 32],
}
