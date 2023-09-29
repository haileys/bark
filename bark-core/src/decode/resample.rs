use core::cmp;
use core::ffi::{c_void, c_int, CStr};
use core::fmt::{self, Debug};
use core::ptr::NonNull;

use bark_protocol::SampleRate;
use bark_protocol::types::AudioFrameF32;
use bark_protocol::time::SampleDuration;

mod ffi {
    use core::ffi::{c_void, c_int, c_char};

    #[link(name = "speexdsp")]
    extern "C" {
        pub fn speex_resampler_init(
            nb_channels: u32,
            in_rate: u32,
            out_rate: u32,
            quality: c_int,
            err: *mut c_int,
        ) -> *mut c_void;

        pub fn speex_resampler_set_rate(
            ptr: *mut c_void,
            in_rate: u32,
            out_rate: u32,
        ) -> c_int;

        pub fn speex_resampler_process_interleaved_float(
            ptr: *mut c_void,
            input: *const f32,
            input_len: *mut u32,
            output: *mut f32,
            output_len: *mut u32,
        ) -> c_int;

        pub fn speex_resampler_destroy(ptr: *mut c_void);

        pub fn speex_resampler_strerror(err: c_int) -> *const c_char;
    }
}

pub struct Resampler {
    ptr: ResamplerPtr,
}

unsafe impl Send for Resampler {}

pub struct ProcessResult {
    /// per-channel
    pub input_read: SampleDuration,
    /// per-channel
    pub output_written: SampleDuration,
}

impl Resampler {
    pub fn new() -> Result<Self, SpeexError> {
        let mut err: c_int = 0;

        let ptr = unsafe {
            ffi::speex_resampler_init(
                bark_protocol::CHANNELS.into(),
                bark_protocol::SAMPLE_RATE.into(),
                bark_protocol::SAMPLE_RATE.into(),
                10,
                &mut err
            )
        };

        let ptr = NonNull::new(ptr)
            .map(ResamplerPtr)
            .ok_or_else(|| SpeexError::from_err(err))?;

        Ok(Resampler { ptr })
    }

    fn as_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    pub fn set_input_rate(&mut self, rate: SampleRate) -> Result<(), SpeexError> {
        let err = unsafe {
            ffi::speex_resampler_set_rate(
                self.as_ptr(),
                rate.0,
                bark_protocol::SAMPLE_RATE.into(),
            )
        };

        if err != 0 {
            return Err(SpeexError::from_err(err));
        }

        Ok(())
    }

    pub fn process_floats(&mut self, input: &[AudioFrameF32], output: &mut [AudioFrameF32])
        -> Result<ProcessResult, SpeexError>
    {
        // speex API takes frame count, our input slices are already
        // represented as whole frames:
        let input_len = input.len();
        let output_len = output.len();

        // usize could technically be 64 bit, speex only takes u32 sizes,
        // we don't want to panic or truncate, so let's just pick a reasonable
        // length and cap input and output since the API allows us to.
        // i'm going to say a reasonable length for a single call is 1<<20.
        let max_reasonable_len = 1 << 20;
        let input_len = cmp::min(input_len, max_reasonable_len);
        let output_len = cmp::min(output_len, max_reasonable_len);

        let mut input_len = u32::try_from(input_len).unwrap();
        let mut output_len = u32::try_from(output_len).unwrap();

        let err = unsafe {
            ffi::speex_resampler_process_interleaved_float(
                self.as_ptr(),
                AudioFrameF32::as_interleaved_slice(input).as_ptr(),
                &mut input_len,
                AudioFrameF32::as_interleaved_slice_mut(output).as_mut_ptr(),
                &mut output_len,
            )
        };

        if err != 0 {
            return Err(SpeexError::from_err(err));
        }

        Ok(ProcessResult {
            input_read: SampleDuration::from_frame_count(u64::from(input_len)),
            output_written: SampleDuration::from_frame_count(u64::from(output_len)),
        })
    }
}

#[repr(transparent)]
struct ResamplerPtr(NonNull<c_void>);

impl ResamplerPtr {
    pub fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for ResamplerPtr {
    fn drop(&mut self) {
        unsafe {
            ffi::speex_resampler_destroy(self.0.as_ptr());
        }
    }
}

pub struct SpeexError(&'static CStr);

impl SpeexError {
    fn from_err(err: c_int) -> Self {
        let cstr = unsafe {
            CStr::from_ptr(ffi::speex_resampler_strerror(err))
        };

        SpeexError(cstr)
    }

    fn message(&self) -> &'static str {
        self.0.to_str().unwrap_or_default()
    }
}

impl Debug for SpeexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SpeexError({:?})", self.message())
    }
}
