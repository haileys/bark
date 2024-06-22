use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::audio::Output;

pub struct OwnedOutput {
    output: Arc<Mutex<Option<Output>>>,
}

impl OwnedOutput {
    pub fn new(output: Output) -> Self {
        Self { output: Arc::new(Mutex::new(Some(output))) }
    }

    /// TODO - this may block for the duration of an alsa_pcm_write
    /// fix this
    pub fn steal(&mut self) -> OutputRef {
        let output = self.output.lock().unwrap().take();
        self.output = Arc::new(Mutex::new(output));

        OutputRef { output: self.output.clone() }
    }
}

#[derive(Clone)]
pub struct OutputRef {
    output: Arc<Mutex<Option<Output>>>,
}

impl OutputRef {
    pub fn lock(&self) -> Option<OutputLock> {
        let guard = self.output.lock().unwrap();

        if guard.is_some() {
            Some(OutputLock { guard })
        } else {
            None
        }
    }
}

pub struct OutputLock<'a> {
    guard: MutexGuard<'a, Option<Output>>,
}

impl<'a> Deref for OutputLock<'a> {
    type Target = Output;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().unwrap()
    }
}

impl<'a> DerefMut for OutputLock<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().unwrap()
    }
}
