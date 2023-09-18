#![no_std]

#[cfg(not(any(feature = "alloc", feature = "pbuf")))]
compile_error!("must enable alloc feature!");

#[derive(Debug, Clone, Copy)]
pub struct AllocError {
    pub requested_bytes: usize
}

#[cfg(feature = "alloc")]
#[path = "alloc_box_impl.rs"]
mod impl_;

#[cfg(feature = "pbuf")]
#[path = "pbuf_impl.rs"]
mod impl_;

pub use impl_::*;
