use std::sync::{Arc, Condvar, Mutex};

use bark_core::receive::queue::{PacketQueue, AudioPts};
use thiserror::Error;

pub struct QueueSender {
    shared: Arc<Shared>,
}

pub struct QueueReceiver {
    shared: Arc<Shared>,
}

struct Shared {
    queue: Mutex<Option<PacketQueue>>,
    notify: Condvar,
}

pub fn channel(queue: PacketQueue) -> (QueueSender, QueueReceiver) {
    let shared = Arc::new(Shared {
        queue: Mutex::new(Some(queue)),
        notify: Condvar::new(),
    });

    let tx = QueueSender { shared: shared.clone() };
    let rx = QueueReceiver { shared: shared.clone() };

    (tx, rx)
}

#[derive(Debug, Clone, Copy, Error)]
#[error("audio receiver thread unexpectedly disconnected")]
pub struct Disconnected;

impl QueueSender {
    pub fn send(&self, packet: AudioPts) -> Result<usize, Disconnected> {
        let mut queue = self.shared.queue.lock().unwrap();

        let Some(queue) = queue.as_mut() else {
            return Err(Disconnected);
        };

        queue.insert_packet(packet);

        self.shared.notify.notify_all();

        Ok(queue.len())
    }
}

impl QueueReceiver {
    pub fn recv(&self) -> Result<Option<AudioPts>, Disconnected> {
        let mut queue_lock = self.shared.queue.lock().unwrap();

        loop {
            let Some(queue) = queue_lock.as_mut() else {
                return Err(Disconnected);
            };

            if queue.len() > 0 {
                return Ok(queue.pop_front());
            }

            // if queue is empty we'll block until notified
            queue_lock = self.shared.notify.wait(queue_lock).unwrap();
            continue;
        }
    }
}
