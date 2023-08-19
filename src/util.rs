use std::mem::MaybeUninit;

use cpal::{StreamConfig, BufferSize, SupportedBufferSize};
use cpal::traits::DeviceTrait;
use libc::SCHED_FIFO;

use crate::RunError;
use crate::protocol;

pub fn set_realtime_priority(priority: i32) {
    let mut param = unsafe {
        let mut param = MaybeUninit::uninit();
        let rc = libc::sched_getparam(0, param.as_mut_ptr());
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            eprintln!("set_thread_realtime: error in sched_get_param: {err}");
            return;
        }
        param.assume_init()
    };

    param.sched_priority = priority;

    let rc = unsafe { libc::sched_setscheduler(0, SCHED_FIFO, &param) };
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        eprintln!("set_realtime_priority: failed to select SCHED_FIFO scheduler: {err}");
    }
}

pub fn config_for_device(device: &cpal::Device) -> Result<StreamConfig, RunError> {
    let configs = device.supported_input_configs()
        .map_err(RunError::StreamConfigs)?;

    let config = configs
        .filter(|config| config.sample_format() == protocol::SAMPLE_FORMAT)
        .filter(|config| config.channels() == protocol::CHANNELS)
        .nth(0)
        .ok_or(RunError::NoSupportedStreamConfig)?;

    let buffer_size = match config.buffer_size() {
        SupportedBufferSize::Range { min, .. } => {
            std::cmp::max(*min, protocol::FRAMES_PER_PACKET as u32)
        }
        SupportedBufferSize::Unknown => {
            protocol::FRAMES_PER_PACKET as u32
        }
    };

    Ok(StreamConfig {
        channels: protocol::CHANNELS,
        sample_rate: protocol::SAMPLE_RATE,
        buffer_size: BufferSize::Fixed(buffer_size),
    })
}
