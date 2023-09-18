use core::fmt::{self, Debug};

use crate::packet::MAX_PACKET_SIZE;

pub use bark_alloc::AllocError;

pub struct PacketBuffer {
    raw: bark_alloc::FixedBuffer<MAX_PACKET_SIZE>,
    len: usize,
}

impl Debug for PacketBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PacketBuffer {{ len = {}; {:x?} }}", self.len, &self.raw[0..self.len])
    }
}

impl PacketBuffer {
    pub fn allocate() -> Result<Self, AllocError> {
        Ok(PacketBuffer {
            raw: bark_alloc::FixedBuffer::alloc_zeroed()?,
            len: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.raw[0..self.len]
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.raw[0..self.len]
    }

    pub fn as_full_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.raw
    }
}
