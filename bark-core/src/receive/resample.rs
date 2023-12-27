use std::ffi::{c_void, c_int, CStr};
use std::fmt::Debug;
use std::ptr;

use bark_protocol::time::SampleDuration;

use self::ffi::speex_resampler_strerror;

mod ffi {
    use std::ffi::{c_void, c_int, c_char};

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
    pub fn new() -> Self {
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

        if ptr == ptr::null_mut() {
            // this should only really fail on allocation error,
            // which rust already makes a panic, so shrug let's
            // just panic so callers don't have to deal with it
            let err = SpeexError::from_err(err);
            panic!("speex_resampler_init failed: {err:?}");
        }

        Resampler { ptr: ResamplerPtr(ptr) }
    }

    pub fn set_input_rate(&mut self, rate: u32) -> Result<(), SpeexError> {
        let err = unsafe {
            ffi::speex_resampler_set_rate(
                self.ptr.0,
                rate,
                bark_protocol::SAMPLE_RATE.into(),
            )
        };

        if err != 0 {
            return Err(SpeexError::from_err(err));
        }

        Ok(())
    }

    pub fn process_interleaved(&mut self, input: &[f32], output: &mut [f32])
        -> Result<ProcessResult, SpeexError>
    {
        // speex API takes frame count:
        let input_len = input.len() / usize::from(bark_protocol::CHANNELS);
        let output_len = output.len() / usize::from(bark_protocol::CHANNELS);

        // usize could technically be 64 bit, speex only takes u32 sizes,
        // we don't want to panic or truncate, so let's just pick a reasonable
        // length and cap input and output since the API allows us to.
        // i'm going to say a reasonable length for a single call is 1<<20.
        let max_reasonable_len = 1 << 20;
        let input_len = std::cmp::min(input_len, max_reasonable_len);
        let output_len = std::cmp::min(output_len, max_reasonable_len);

        let mut input_len = u32::try_from(input_len).unwrap();
        let mut output_len = u32::try_from(output_len).unwrap();

        let err = unsafe {
            ffi::speex_resampler_process_interleaved_float(
                self.ptr.0,
                input.as_ptr(),
                &mut input_len,
                output.as_mut_ptr(),
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
struct ResamplerPtr(*mut c_void);

impl Drop for ResamplerPtr {
    fn drop(&mut self) {
        unsafe {
            ffi::speex_resampler_destroy(self.0);
        }
    }
}

pub struct SpeexError(&'static CStr);

impl SpeexError {
    fn from_err(err: c_int) -> Self {
        let cstr = unsafe {
            CStr::from_ptr(speex_resampler_strerror(err))
        };

        SpeexError(cstr)
    }
}

impl Debug for SpeexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SpeexError({:?})", self.0.to_string_lossy())
    }
}
