use bark_protocol::FRAMES_PER_PACKET;

pub const MAX_QUEUED_DECODE_SEGMENTS: usize = 48;
pub const DECODE_BUFFER_FRAMES: usize = FRAMES_PER_PACKET * 2;
