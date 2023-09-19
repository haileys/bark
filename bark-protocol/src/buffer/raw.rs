#[cfg(feature = "alloc")]
#[path = "alloc_impl.rs"]
mod impl_;

#[cfg(feature = "pbuf")]
#[path = "pbuf_impl.rs"]
mod impl_;

pub use impl_::*;
