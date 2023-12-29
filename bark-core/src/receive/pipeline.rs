use bark_protocol::FRAMES_PER_PACKET;
use bytemuck::Zeroable;

use bark_protocol::packet::Audio;
use bark_protocol::types::AudioPacketHeader;

use crate::audio::Frame;
use crate::decode::Decoder;
use crate::receive::resample::Resampler;
use crate::receive::timing::{RateAdjust, Timing};

pub struct Pipeline {
    /// None indicates error creating decoder, we cannot decode this stream
    decoder: Option<Decoder>,
    resampler: Resampler,
    rate_adjust: RateAdjust,
}

impl Pipeline {
    pub fn new(header: &AudioPacketHeader) -> Self {
        let decoder = match Decoder::new(header) {
            Ok(dec) => {
                log::info!("instantiated decoder for new stream: {}", dec.describe());
                Some(dec)
            }
            Err(err) => {
                log::error!("error creating decoder for new stream: {err}");
                None
            }
        };

        Pipeline {
            decoder,
            resampler: Resampler::new(),
            rate_adjust: RateAdjust::new(),
        }
    }

    pub fn slew(&self) -> bool {
        self.rate_adjust.slew()
    }

    pub fn set_timing(&mut self, timing: Timing) {
        let rate = self.rate_adjust.sample_rate(timing);
        let _ = self.resampler.set_input_rate(rate.0);
    }

    pub fn process(&mut self, packet: Option<&Audio>, out: &mut [Frame]) -> usize {
        // decode packet
        let mut decode_buffer = [Frame::zeroed(); FRAMES_PER_PACKET];

        if let Some(decoder) = self.decoder.as_mut() {
            match decoder.decode(packet, &mut decode_buffer) {
                Ok(()) => {}
                Err(e) => {
                    log::warn!("error in decoder, skipping packet: {e}");
                    decode_buffer.fill(Frame::zeroed());
                }
            }
        }

        // resample decoded audio
        let resample = self.resampler.process(&decode_buffer, out)
            .expect("resample error!");

        assert_eq!(resample.input_read.0, decode_buffer.len());

        resample.output_written.0
    }
}
