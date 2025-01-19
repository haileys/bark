use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

use bark_core::audio::Format;

use crate::audio::Output;

pub struct OwnedOutput<F: Format> {
    output: Arc<Mutex<Option<Output<F>>>>,
}

impl<F: Format> OwnedOutput<F> {
    pub fn new(output: Output<F>) -> Self {
        Self { output: Arc::new(Mutex::new(Some(output))) }
    }

    /// TODO - this may block for the duration of an alsa_pcm_write
    /// fix this
    pub fn steal(&mut self) -> OutputRef<F> {
        let output = self.output.lock().unwrap().take();
        self.output = Arc::new(Mutex::new(output));

        OutputRef { output: self.output.clone() }
    }
}

#[derive(Clone)]
pub struct OutputRef<F: Format> {
    output: Arc<Mutex<Option<Output<F>>>>,
}

impl<F: Format> OutputRef<F> {
    pub fn lock(&self) -> Option<OutputLock<F>> {
        let guard = self.output.lock().unwrap();

        if guard.is_some() {
            Some(OutputLock { guard })
        } else {
            None
        }
    }
}

pub struct OutputLock<'a, F: Format> {
    guard: MutexGuard<'a, Option<Output<F>>>,
}

impl<'a, F: Format> Deref for OutputLock<'a, F> {
    type Target = Output<F>;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().unwrap()
    }
}

impl<'a, F: Format> DerefMut for OutputLock<'a, F> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().unwrap()
    }
}
