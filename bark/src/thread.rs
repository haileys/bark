use std::ffi::CString;
use std::io::ErrorKind;
use std::sync::atomic::AtomicBool;

pub fn set_name(name: &str) {
    let cstr = CString::new(name)
        .expect("not a cstring in set_thread_name");

    unsafe {
        libc::pthread_setname_np(libc::pthread_self(), cstr.as_ptr());
    }
}

pub fn set_realtime_priority() {
    let rc = unsafe {
        libc::sched_setscheduler(
            0,
            libc::SCHED_FIFO,
            &libc::sched_param {
                sched_priority: 99,
            }
        )
    };

    if rc < 0 {
        static WARNED: AtomicBool = AtomicBool::new(false);
        let warned = WARNED.swap(true, std::sync::atomic::Ordering::Relaxed);

        if !warned {
            let err = std::io::Error::last_os_error();

            log::warn!("failed to set realtime thread priority: {err}");

            if err.kind() == ErrorKind::PermissionDenied {
                let path = std::env::current_exe()
                    .map(|path| path.display().to_string());

                let path = path.as_ref()
                    .map(|path| path.as_str())
                    .unwrap_or("path/to/bark");

                log::warn!("fix by running: setcap cap_sys_nice=ep {path}")
            }
        }
    }

}
