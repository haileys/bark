use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use bark_protocol::SAMPLE_RATE;
use bytemuck::Zeroable;
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::render_callback::{data, Args};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, SampleFormat, Scope};

use bark_core::audio::Frame;
use bark_protocol::time::SampleDuration;

use crate::audio::DeviceOpt;
use crate::audio::coreaudio::Disconnected;

pub struct Output {
    _unit: AudioUnit,
    tx: Buffer,
}

impl Output {
    pub fn new(opt: DeviceOpt) -> Result<Self, coreaudio::Error> {
        if let Some(device_name) = opt.device {
            log::warn!("ignoring device setting {device_name:?} on macOS, using default output");
        }

        let mut unit = AudioUnit::new(IOType::DefaultOutput)?;

        unit.set_property(
            coreaudio_sys::kAudioUnitProperty_SampleRate,
            Scope::Input,
            Element::Output,
            Some(&f64::from(SAMPLE_RATE)),
        )?;

        let period = u32::try_from(opt.period.to_frame_count()).unwrap();

        unit.set_property(
            coreaudio_sys::kAudioUnitProperty_MaximumFramesPerSlice,
            Scope::Input,
            Element::Output,
            Some(&period),
        )?;

        let format = unit.input_stream_format()?;
        log::info!("audio format: {:?}", format);
        assert_eq!(format.sample_format, SampleFormat::F32);
        assert_eq!(format.channels, 2);
        assert_eq!(format.flags,
            LinearPcmFlags::IS_FLOAT |
            LinearPcmFlags::IS_PACKED |
            LinearPcmFlags::IS_NON_INTERLEAVED
        );

        let buffer = Buffer::new(usize::try_from(opt.buffer.to_frame_count()).unwrap());

        unit.set_render_callback(non_interleaved_callback(buffer.clone()))?;
        unit.start()?;

        Ok(Output {
            _unit: unit,
            tx: buffer,
        })
    }

    pub fn write(&mut self, audio: &[Frame]) -> Result<(), Disconnected> {
        log::trace!("will write {} frames", audio.len());
        let result = self.tx.write(audio);
        log::trace!("did write {} frames", audio.len());
        result
    }

    pub fn delay(&self) -> Result<SampleDuration, Disconnected> {
        Ok(SampleDuration::from_frame_count(self.tx.len() as u64))
    }
}

fn non_interleaved_callback(rx: Buffer)
    -> impl FnMut(Args<data::NonInterleaved<f32>>) -> Result<(), ()>
{
    let mut buffer = Vec::<Frame>::new();
    let mut tot = 0;

    move |mut args: Args<data::NonInterleaved<f32>>| {
        tot += args.num_frames;
        log::trace!("want {} frames (total = {tot}) (bus = {})", args.num_frames, args.bus_number);

        // set buffer size appropriately, reusing the allocation
        buffer.resize(args.num_frames, Frame::zeroed());

        // read from ring buffer
        let n = rx.read(bytemuck::cast_slice_mut(&mut buffer))
            .map_err(|_: Disconnected| ())?;

        // zero out any part of the buffer that wasn't read
        let (_ready, missing) = buffer.split_at_mut(n);
        missing.fill(Frame::zeroed());

        if missing.len() > 0 {
            log::warn!("underrun!");
        }

        // copy audio out
        for (channel, samples) in args.data.channels_mut().enumerate() {
            match channel {
                0 => { copy_from_iter(samples, buffer.iter().map(|buffer| buffer.0)); }
                1 => { copy_from_iter(samples, buffer.iter().map(|buffer| buffer.1)); }
                _ => { samples.fill(0.0); }
            }

        }

        Ok(())
    }
}

#[derive(Clone)]
struct Buffer {
    shared: Arc<BufferShared>,
}

struct BufferShared {
    deque: Mutex<VecDeque<Frame>>,
    cond: Condvar,
    size: usize,
}

impl Buffer {
    pub fn new(size: usize) -> Self {
        Buffer {
            shared: Arc::new(BufferShared {
                deque: Mutex::new(VecDeque::new()),
                cond: Condvar::new(),
                size,
            })
        }
    }

    pub fn len(&self) -> usize {
        self.shared.deque.lock().unwrap().len()
    }

    pub fn read(&self, out: &mut [Frame]) -> Result<usize, Disconnected> {
        if Arc::strong_count(&self.shared) == 1 {
            return Err(Disconnected);
        }

        let mut buffer = self.shared.deque.lock().unwrap();

        let n = std::cmp::min(buffer.len(), out.len());

        out[..n].fill_with(|| buffer.pop_front().unwrap());

        self.shared.cond.notify_all();

        log::trace!("buffer read: n={n}");
        Ok(n)
    }

    pub fn write(&self, mut data: &[Frame]) -> Result<(), Disconnected> {
        if data.len() == 0 {
            return Ok(());
        }

        let shared = &self.shared;

        let mut buffer = shared.deque.lock().unwrap();

        loop {
            if Arc::strong_count(shared) == 1 {
                return Err(Disconnected);
            }

            let available = shared.size - buffer.len();
            let n = std::cmp::min(available, data.len());

            let (write, next) = data.split_at(n);

            for frame in write {
                buffer.push_back(*frame);
            }

            if next.is_empty() {
                log::trace!("buffer write: returning");
                return Ok(());
            }

            log::trace!("buffer write: waiting");
            buffer = shared.cond.wait(buffer).unwrap();
            log::trace!("buffer write: continuing");
            data = next;
        }
    }
}

fn copy_from_iter<T>(slice: &mut [T], mut iter: impl Iterator<Item = T>) {
    slice.fill_with(|| iter.next()
        .expect("iter shorter than slice in copy_from_iter"));

    if iter.next().is_some() {
        panic!("iter longer than slice in copy_from_iter");
    }
}
