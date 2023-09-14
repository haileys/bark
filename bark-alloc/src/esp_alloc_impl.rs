use core::alloc::{GlobalAlloc, Layout};
use core::ops::{Deref, DerefMut};
use core::ptr::null_mut;
use core::slice;
use core::sync::atomic::{AtomicPtr, Ordering};

use esp_alloc::EspHeap;

static HEAP: AtomicPtr<EspHeap> = AtomicPtr::new(null_mut());

pub unsafe fn set_heap(heap: &'static EspHeap) {
    let result = HEAP.compare_exchange(
        null_mut(),
        heap as *const _ as *mut _,
        Ordering::SeqCst,
        Ordering::Relaxed,
    );

    if result.is_err() {
        panic!("bark_alloc: attempted to call set_heap twice");
    }
}

fn heap() -> &'static EspHeap {
    let ptr = HEAP.load(Ordering::Relaxed);
    if ptr == null_mut() {
        panic!("bark_alloc: heap accessed before set! call set_heap first.")
    }

    unsafe { &*(ptr as *const _) }
}

#[repr(transparent)]
pub struct FixedBuffer<const N: usize>(*mut u8);

impl<const N: usize> FixedBuffer<N> {
    const LAYOUT: Layout = unsafe {
        // Layout::from_size_align is const but returns a Result,
        // we can't const unwrap results on stable rust yet.
        Layout::from_size_align_unchecked(N, 4)
    };

    pub fn alloc_zeroed() -> Self {
        let ptr = unsafe { heap().alloc_zeroed(Self::LAYOUT) };
        if ptr == null_mut() {
            panic!("bark_alloc: allocation failed! requsted size: {N}");
        }

        FixedBuffer(ptr)
    }
}

impl<const N: usize> Drop for FixedBuffer<N> {
    fn drop(&mut self) {
        unsafe { heap().dealloc(self.0, Self::LAYOUT); }
    }
}

impl<const N: usize> Deref for FixedBuffer<N> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.0, N) }
    }
}

impl<const N: usize> DerefMut for FixedBuffer<N> {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.0, N) }
    }
}
