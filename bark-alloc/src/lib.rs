#![no_std]

#[cfg(not(any(feature = "alloc", feature = "esp_alloc")))]
compile_error!("must enable alloc feature!");

#[cfg(feature = "alloc")]
#[path = "alloc_box_impl.rs"]
mod impl_;

#[cfg(feature = "esp_alloc")]
#[path = "esp_alloc_impl.rs"]
mod impl_;

pub use impl_::*;
