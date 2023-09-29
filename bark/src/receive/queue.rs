use std::sync::{Arc, Mutex};

use bark_core::receive::queue::PacketQueue as PacketQueueCore;
use bark_protocol::packet::Audio;

#[derive(Clone)]
pub struct PacketQueue {
    queue: Arc<Mutex<PacketQueueCore>>,
}

impl PacketQueue {
    pub fn disconnected(&self) -> bool {
        Arc::strong_count(&self.queue) == 1
    }

    pub fn new(start_seq: u64) -> Self {
        PacketQueue {
            queue: Arc::new(Mutex::new(PacketQueueCore::new(start_seq)))
        }
    }

    pub async fn receive_packet(&self, packet: Audio) {
        let mut queue = self.queue.lock().unwrap();
        queue.insert_packet(packet);
    }

    pub async fn pop_front(&self) -> Option<Audio> {
        let mut queue = self.queue.lock().unwrap();
        queue.pop_front()
    }
}
