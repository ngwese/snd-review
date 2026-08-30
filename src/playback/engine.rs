use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use super::provider::PlaybackDataProvider;
use super::transport::TransportState;

const IN_OUT_NONE: usize = usize::MAX;

pub struct PlaybackShared {
    pub provider: Arc<dyn PlaybackDataProvider>,
    pub position: AtomicUsize,
    pub transport: AtomicU8,
    pub looping: std::sync::atomic::AtomicBool,
    pub in_point: AtomicUsize,
    pub out_point: AtomicUsize,
    source_rate: u32,
    output_rate: u32,
}

impl PlaybackShared {
    pub fn new(provider: Arc<dyn PlaybackDataProvider>, output_rate: u32) -> Self {
        Self {
            source_rate: provider.sample_rate(),
            output_rate,
            provider,
            position: AtomicUsize::new(0),
            transport: AtomicU8::new(TransportState::Stopped.to_u8()),
            looping: std::sync::atomic::AtomicBool::new(false),
            in_point: AtomicUsize::new(IN_OUT_NONE),
            out_point: AtomicUsize::new(IN_OUT_NONE),
        }
    }

    pub fn set_transport(&self, state: TransportState) {
        self.transport.store(state.to_u8(), Ordering::SeqCst);
    }

    pub fn transport(&self) -> TransportState {
        TransportState::from_u8(self.transport.load(Ordering::SeqCst))
    }

    pub fn set_position(&self, sample: usize) {
        self.position.store(sample, Ordering::SeqCst);
    }

    pub fn position(&self) -> usize {
        self.position.load(Ordering::SeqCst)
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping, Ordering::SeqCst);
    }

    pub fn set_in_out(&self, in_point: Option<usize>, out_point: Option<usize>) {
        self.in_point.store(in_point.unwrap_or(IN_OUT_NONE), Ordering::SeqCst);
        self.out_point
            .store(out_point.unwrap_or(IN_OUT_NONE), Ordering::SeqCst);
    }

    fn playback_start(&self) -> usize {
        let v = self.in_point.load(Ordering::SeqCst);
        if v == IN_OUT_NONE { 0 } else { v }
    }

    fn playback_end(&self) -> usize {
        let looping = self.looping.load(Ordering::SeqCst);
        if looping {
            let v = self.out_point.load(Ordering::SeqCst);
            if v == IN_OUT_NONE {
                self.provider.frames().saturating_sub(1)
            } else {
                v
            }
        } else {
            self.provider.frames().saturating_sub(1)
        }
    }

    fn fill_output(&self, output: &mut [f32]) {
        output.fill(0.0);
        if self.transport() != TransportState::Playing {
            return;
        }

        let channels = self.provider.channel_count();
        if channels == 0 {
            return;
        }
        let out_frames = output.len() / channels;
        if out_frames == 0 {
            return;
        }

        let end = self.playback_end();
        let start_bound = self.playback_start();
        let looping = self.looping.load(Ordering::SeqCst);
        let step = self.source_rate as f64 / self.output_rate as f64;

        let mut pos_f = self.position.load(Ordering::SeqCst) as f64;
        let mut frame_buf = vec![0.0f32; channels];

        for out_frame in 0..out_frames {
            let source_pos = pos_f as usize;
            if source_pos > end {
                if looping && end > start_bound {
                    pos_f = start_bound as f64;
                    continue;
                }
                self.set_transport(TransportState::Stopped);
                self.position.store(end, Ordering::SeqCst);
                break;
            }

            self.provider
                .read_interleaved(source_pos, 1, &mut frame_buf);
            for ch in 0..channels {
                output[out_frame * channels + ch] = frame_buf[ch];
            }
            pos_f += step;
        }

        let final_pos = pos_f.min(end as f64) as usize;
        self.position.store(final_pos, Ordering::SeqCst);
    }
}

pub struct PlaybackEngine {
    _stream: Stream,
    pub shared: Arc<PlaybackShared>,
}

impl PlaybackEngine {
    pub fn open(device: &Device, provider: Arc<dyn PlaybackDataProvider>) -> Result<Self> {
        let channels = provider.channel_count().max(1) as u16;
        let source_rate = provider.sample_rate();

        let default_config = device
            .default_output_config()
            .context("failed to get default output config")?;

        let sample_format = default_config.sample_format();
        let mut stream_config = StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(source_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let supports_source_rate = device
            .supported_output_configs()?
            .any(|c| {
                c.channels() == channels
                    && c.min_sample_rate().0 <= source_rate
                    && c.max_sample_rate().0 >= source_rate
            });

        if !supports_source_rate {
            stream_config.sample_rate = default_config.sample_rate();
        }

        let output_rate = stream_config.sample_rate.0;
        let shared = Arc::new(PlaybackShared::new(provider, output_rate));
        let shared_cb = shared.clone();

        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| shared_cb.fill_output(data),
                |_| {},
                None,
            ),
            SampleFormat::I16 => {
                let shared_cb = shared.clone();
                device.build_output_stream(
                    &stream_config,
                    move |data: &mut [i16], _| {
                        let mut temp = vec![0.0f32; data.len()];
                        shared_cb.fill_output(&mut temp);
                        for (out, sample) in data.iter_mut().zip(temp) {
                            *out = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        }
                    },
                    |_| {},
                    None,
                )
            }
            other => anyhow::bail!("unsupported output sample format: {other:?}"),
        }
        .context("failed to build output stream")?;

        stream.play().context("failed to start output stream")?;

        Ok(Self {
            _stream: stream,
            shared,
        })
    }
}
