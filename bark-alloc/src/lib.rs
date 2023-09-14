#![no_std]

#[cfg(not(feature = "alloc"))]
compile_error!("must enable alloc feature!");

#[cfg(feature = "alloc")]
mod impl_ {
    extern crate alloc;

    use derive_more::{Deref, DerefMut};

    #[repr(transparent)]
    #[derive(Deref, DerefMut)]
    #[deref(forward)]
    pub struct FixedBuffer<const N: usize>(alloc::boxed::Box<[u8]>);

    impl<const N: usize> FixedBuffer<N> {
        pub fn alloc_zeroed() -> Self {
            FixedBuffer(bytemuck::allocation::zeroed_slice_box(N))
        }
    }
}

pub use impl_::*;
