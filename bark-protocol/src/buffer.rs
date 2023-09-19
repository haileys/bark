use core::fmt::{self, Debug};

#[cfg(feature = "alloc")]
#[path = "buffer/alloc_impl.rs"]
pub mod alloc;
#[cfg(feature = "alloc")]
use alloc as impl_;

#[cfg(feature = "pbuf")]
#[path = "buffer/pbuf_impl.rs"]
pub mod pbuf;
#[cfg(feature = "pbuf")]
use pbuf as impl_;

pub use impl_::{RawBuffer, BufferImpl};

#[derive(Debug, Copy, Clone)]
pub struct AllocError {
    pub requested_bytes: usize,
}

#[repr(transparent)]
pub struct PacketBuffer {
    underlying: BufferImpl,
}

impl Debug for PacketBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PacketBuffer {{ len = {}; {:x?} }}", self.len(), &self.as_bytes())
    }
}

impl PacketBuffer {
    pub fn allocate(len: usize) -> Result<Self, AllocError> {
        Ok(PacketBuffer {
            underlying: BufferImpl::allocate_zeroed(len)?,
        })
    }

    pub fn from_underlying(underlying: BufferImpl) -> Self {
        PacketBuffer { underlying }
    }

    pub fn len(&self) -> usize {
        self.underlying.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.underlying.bytes()
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        self.underlying.bytes_mut()
    }
}
