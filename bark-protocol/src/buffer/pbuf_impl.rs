use core::ptr::NonNull;

use crate::buffer::AllocError;

pub type RawBuffer = NonNull<ffi::pbuf>;

#[repr(transparent)]
pub struct BufferImpl(RawBuffer);

unsafe impl Send for BufferImpl {}
unsafe impl Sync for BufferImpl {}

impl BufferImpl {
    pub fn allocate_zeroed(len: usize) -> Result<Self, AllocError> {
        let mut pbuf = alloc_uninit_pbuf(len)
            .ok_or(AllocError { requested_bytes: len })?;

        // SAFETY: pbuf payload ptr always points to buffer of size len
        unsafe {
            let payload = pbuf.as_mut().payload as *mut u8;
            core::ptr::write_bytes(payload, 0, len);
        }

        Ok(BufferImpl(pbuf))
    }

    pub unsafe fn from_raw(pbuf: RawBuffer) -> Self {
        BufferImpl(pbuf)
    }

    pub fn len(&self) -> usize {
        usize::from(self.pbuf().len)
    }

    pub fn bytes(&self) -> &[u8] {
        let len = self.len();
        let payload = self.pbuf().payload as *const _;

        // SAFETY: pbuf payload ptr always points to buffer of size len
        unsafe { core::slice::from_raw_parts(payload, len) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        let payload = self.pbuf_mut().payload;

        // SAFETY: pbuf payload ptr always points to a buffer of size len
        unsafe { core::slice::from_raw_parts_mut(payload, len) }
    }

    pub fn pbuf(&self) -> &ffi::pbuf {
        // SAFETY: this struct owns ffi::pbuf
        unsafe { self.0.as_ref() }
    }

    pub fn pbuf_mut(&mut self) -> &mut ffi::pbuf {
        // SAFETY: this struct owns ffi::pbuf
        unsafe { self.0.as_mut() }
    }
}

fn alloc_uninit_pbuf(len: usize) -> Option<NonNull<ffi::pbuf>> {
    let len = u16::try_from(len).ok()?;

    // SAFETY: calls an alloc function, always safe
    let ptr = unsafe {
        ffi::pbuf_alloc(ffi::PBUF_TRANSPORT as i32, len, ffi::PBUF_RAM)
    };

    NonNull::new(ptr)
}


impl Drop for BufferImpl {
    fn drop(&mut self) {
        // SAFETY: we own pbuf
        unsafe { ffi::pbuf_free(self.0.as_ptr()); }
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
    pub const PBUF_TRANSPORT: usize = 74;

    /// Downstream crates should statically assert that this is equal to their
    /// PBUF_RAM constant
    pub const PBUF_RAM: u32 = PBUF_ALLOC_FLAG_DATA_CONTIGUOUS | PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS | PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP;


    #[repr(C)]
    pub struct pbuf {
        pub next: *mut pbuf,
        pub payload: *mut u8,
        pub tot_len: u16,
        pub len: u16,
        // fields continue but this is all we need
    }

    extern "C" {
        pub fn pbuf_alloc(layer: i32, length: u16, type_: u32) -> *mut pbuf;
        pub fn pbuf_free(p: *mut pbuf) -> u8;
    }
}
