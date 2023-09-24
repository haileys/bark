pub use esp_pbuf::{Pbuf, PbufMut, AllocatePbufError};
use esp_pbuf::PbufUninit;

pub type RawBuffer = esp_pbuf::PbufMut;
pub type AllocError = AllocatePbufError;

#[repr(transparent)]
pub struct BufferImpl(RawBuffer);

unsafe impl Send for BufferImpl {}
unsafe impl Sync for BufferImpl {}

impl BufferImpl {
    pub fn allocate_zeroed(len: usize) -> Result<Self, AllocatePbufError> {
        let pbuf = PbufUninit::allocate(ffi::PBUF_TRANSPORT, ffi::PBUF_RAM, len)?;
        Ok(BufferImpl(pbuf.zeroed()))
    }

    pub fn from_raw(pbuf: RawBuffer) -> Self {
        BufferImpl(pbuf)
    }

    pub fn len(&self) -> usize {
        self.pbuf().len()
    }

    pub fn bytes(&self) -> &[u8] {
        self.pbuf().bytes()
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        self.pbuf_mut().bytes_mut()
    }

    pub fn pbuf(&self) -> &Pbuf {
        &self.0
    }

    pub fn pbuf_mut(&mut self) -> &mut Pbuf {
        &mut self.0
    }
}

// bindings to esp-lwip pbuf.h
// https://github.com/espressif/esp-lwip/blob/7896c6cad020d17a986f7e850f603e084e319328/src/include/lwip/pbuf.h
pub mod ffi {
    const PBUF_ALLOC_FLAG_DATA_CONTIGUOUS: u32 = 0x0200;
    const PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS: u32 = 0x80;
    const PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP: u32 = 0x00;

    /// Downstream crates should statically assert that this is equal to or
    /// larger than their PBUF_TRANSPORT constant
    pub const PBUF_TRANSPORT: u32 = 74;

    /// Downstream crates should statically assert that this is equal to their
    /// PBUF_RAM constant
    pub const PBUF_RAM: u32 = PBUF_ALLOC_FLAG_DATA_CONTIGUOUS | PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS | PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP;
}
