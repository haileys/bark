use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use crate::AllocError;

#[repr(transparent)]
pub struct FixedBuffer<const N: usize> {
    pbuf: NonNull<ffi::pbuf>,
}

unsafe impl<const N: usize> Send for FixedBuffer<N> {}
unsafe impl<const N: usize> Sync for FixedBuffer<N> {}

impl<const N: usize> FixedBuffer<N> {
    pub fn alloc_zeroed() -> Result<Self, AllocError> {
        let err = AllocError { requested_bytes: ffi::PBUF_TRANSPORT + N };
        let len = u16::try_from(N).map_err(|_| err)?;

        // alloc the pbuf:
        let pbuf = unsafe {
            ffi::pbuf_alloc(ffi::PBUF_TRANSPORT as i32, len, ffi::PBUF_RAM)
        };

        let mut pbuf = NonNull::new(pbuf).ok_or(err)?;

        // zero its contents:
        unsafe {
            let payload = pbuf.as_mut().payload as *mut u8;
            core::ptr::write_bytes(payload, 0, N);
        }

        Ok(FixedBuffer { pbuf })
    }
}

impl<const N: usize> Deref for FixedBuffer<N> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe {
            let pbuf = self.pbuf.as_ref();
            let payload = pbuf.payload as *mut u8 as *const u8;
            let len = usize::from(pbuf.len);
            core::slice::from_raw_parts(payload, len)
        }
    }
}

impl<const N: usize> DerefMut for FixedBuffer<N> {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            let pbuf = self.pbuf.as_mut();
            let payload = pbuf.payload as *mut u8;
            let len = usize::from(pbuf.len);
            core::slice::from_raw_parts_mut(payload, len)
        }
    }
}

impl<const N: usize> Drop for FixedBuffer<N> {
    fn drop(&mut self) {
        unsafe {
            ffi::pbuf_free(self.pbuf.as_ptr());
        }
    }
}

// bindings to esp-lwip pbuf.h
// https://github.com/espressif/esp-lwip/blob/7896c6cad020d17a986f7e850f603e084e319328/src/include/lwip/pbuf.h
pub mod ffi {
    use core::ffi::c_void;

    const PBUF_ALLOC_FLAG_DATA_CONTIGUOUS: i32 = 0x0200;
    const PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS: i32 = 0x80;
    const PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP: i32 = 0x00;

    /// Downstream crates should statically assert that this is equal to or
    /// larger than their PBUF_TRANSPORT constant
    pub const PBUF_TRANSPORT: usize = 74;

    /// Downstream crates should statically assert that this is equal to their
    /// PBUF_RAM constant
    pub const PBUF_RAM: i32 = PBUF_ALLOC_FLAG_DATA_CONTIGUOUS | PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS | PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP;


    #[repr(C)]
    pub struct pbuf {
        pub next: *mut pbuf,
        pub payload: *mut c_void,
        pub tot_len: u16,
        pub len: u16,
        pub type_internal: u8,
        pub flags: u8,
        // fields continue but this is all we need
    }

    extern "C" {
        pub fn pbuf_alloc(layer: i32, length: u16, type_: i32) -> *mut pbuf;
        pub fn pbuf_free(p: *mut pbuf) -> u8;
    }
}
