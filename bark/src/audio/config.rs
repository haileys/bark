use bark_protocol::time::SampleDuration;

pub const DEFAULT_PERIOD: SampleDuration = SampleDuration::from_frame_count(120);
pub const DEFAULT_BUFFER: SampleDuration = SampleDuration::from_frame_count(360);

pub struct DeviceOpt {
    pub device: Option<String>,
    pub period: SampleDuration,
    pub buffer: SampleDuration,
}
