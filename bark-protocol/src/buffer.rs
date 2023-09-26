use core::fmt::{self, Debug};

#[cfg(target_os = "espidf")]
#[path = "buffer/pbuf_impl.rs"]
pub mod pbuf;
#[cfg(target_os = "espidf")]
use pbuf as impl_;

#[cfg(not(target_os = "espidf"))]
#[path = "buffer/alloc_impl.rs"]
pub mod alloc;
#[cfg(not(target_os = "espidf"))]
use alloc as impl_;

pub use impl_::{RawBuffer, BufferImpl};

#[derive(Debug, Copy, Clone)]
pub struct AllocError(pub impl_::AllocError);

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
            underlying: BufferImpl::allocate_zeroed(len)
                .map_err(AllocError)?,
        })
    }

    pub fn from_raw(raw: RawBuffer) -> Self {
        PacketBuffer { underlying: BufferImpl::from_raw(raw) }
    }

    pub fn underlying(&self) -> &BufferImpl {
        &self.underlying
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
