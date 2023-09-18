extern crate alloc;

use derive_more::{Deref, DerefMut};

#[repr(transparent)]
#[derive(Deref, DerefMut)]
#[deref(forward)]
pub struct FixedBuffer<const N: usize>(alloc::boxed::Box<[u8]>);

impl<const N: usize> FixedBuffer<N> {
    pub fn alloc_zeroed() -> Result<Self, crate::AllocError> {
        Ok(FixedBuffer(bytemuck::allocation::zeroed_slice_box(N)))
    }
}
