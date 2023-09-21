extern crate alloc;

pub type RawBuffer = alloc::vec::Vec<u8>;
pub type AllocError = core::convert::Infallible;

#[repr(transparent)]
pub struct BufferImpl(RawBuffer);

impl BufferImpl {
    pub fn allocate_zeroed(len: usize) -> Result<Self, AllocError> {
        let mut vec = RawBuffer::with_capacity(len);
        vec.resize(len, 0);
        Ok(BufferImpl(vec))
    }

    pub fn from_raw(vec: RawBuffer) -> Self {
        BufferImpl(vec)
    }

    pub fn into_raw(self) -> RawBuffer {
        self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
