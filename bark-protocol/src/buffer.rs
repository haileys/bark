use core::fmt::{self, Debug};

pub mod raw;
pub use raw::RawBuffer;

use raw::BufferImpl;

#[derive(Debug, Copy, Clone)]
pub struct AllocError {
    pub requested_bytes: usize,
}

pub struct PacketBuffer {
    raw: BufferImpl,
}

impl Debug for PacketBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PacketBuffer {{ len = {}; {:x?} }}", self.len(), &self.as_bytes())
    }
}

impl PacketBuffer {
    pub fn allocate(len: usize) -> Result<Self, AllocError> {
        Ok(PacketBuffer {
            raw: BufferImpl::allocate_zeroed(len)?,
        })
    }

    pub fn from_raw(raw: RawBuffer) -> Self {
        PacketBuffer { raw: BufferImpl::from_raw(raw) }
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.raw.bytes()
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        self.raw.bytes_mut()
    }
}
