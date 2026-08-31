// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::{fs, fs::File, path::Path, time::SystemTime};

use anyhow::{bail, Context, Result};
use symphonia::core::{
    audio::{AudioBufferRef, SampleBuffer, SignalSpec},
    codecs::{CodecParameters, DecoderOptions, CODEC_TYPE_NULL},
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    sample::SampleFormat,
};

use crate::components::waveform::WaveformDataProvider;
use crate::model::{Buffer, BufferSource};

/// Number of samples folded into each overview peak bin.
pub const PEAK_BLOCK: usize = 256;

/// Decoded, planar audio ready for waveform display.
#[derive(Debug)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
    pub peaks: Vec<Vec<(f32, f32)>>,
}

impl DecodedAudio {
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / f64::from(self.sample_rate)
        }
    }
}

impl WaveformDataProvider for DecodedAudio {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channel_count(&self) -> usize {
        self.channels.len()
    }

    fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }

    fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / f64::from(self.sample_rate)
        }
    }

    fn channel_label(&self, channel: usize) -> String {
        match (self.channels.len(), channel) {
            (1, 0) => "Mono".into(),
            (2, 0) => "L".into(),
            (2, 1) => "R".into(),
            _ => format!("Ch {}", channel + 1),
        }
    }

    fn channel_samples(&self, channel: usize) -> &[f32] {
        &self.channels[channel]
    }

    fn channel_peaks(&self, channel: usize) -> &[(f32, f32)] {
        &self.peaks[channel]
    }
}

struct DecodeMeta {
    container_format: String,
    codec: String,
    bits_per_sample: Option<u32>,
}

/// Decode `path` into planar f32 samples, one vector per channel.
pub fn decode(path: &Path) -> Result<DecodedAudio> {
    decode_with_meta(path).map(|(audio, _)| audio)
}

/// Load a buffer with file metadata from `path`.
pub fn load_buffer(path: &Path) -> Result<Buffer> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    let (audio, meta) = decode_with_meta(path)?;
    Ok(Buffer {
        audio,
        source: Some(BufferSource {
            path: path.to_path_buf(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            size_bytes: metadata.len(),
            bits_per_sample: meta.bits_per_sample,
            container_format: meta.container_format,
            codec: meta.codec,
        }),
        regions: Vec::new(),
        markers: Vec::new(),
    })
}

fn decode_with_meta(path: &Path) -> Result<(DecodedAudio, DecodeMeta)> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("unsupported or unreadable audio format: {}", path.display()))?;

    let mut format = probed.format;
    let container_format = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_string();

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .context("no supported audio track in file")?;

    let codec = track.codec_params.codec.to_string();
    let mut bits_per_sample = codec_bits_per_sample(&track.codec_params);

    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("unsupported audio codec")?;

    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut spec: Option<SignalSpec> = None;
    let mut channels: Vec<Vec<f32>> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::ResetRequired) => {
                bail!("media reset required; this file is not supported");
            }
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => {
                bail!("error reading audio packet: {err}");
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                append_decoded(decoded, &mut sample_buf, &mut spec, &mut channels)?;
            }
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => continue,
            Err(err) => bail!("unrecoverable decode error: {err}"),
        }
    }

    if channels.is_empty() || channels.iter().all(|c| c.is_empty()) {
        bail!("file contained no audio samples");
    }

    let sample_rate = spec.map(|s| s.rate).unwrap_or(0);
    if sample_rate == 0 {
        bail!("audio track has no sample rate");
    }

    let peaks = channels.iter().map(|ch| build_peaks(ch)).collect();
    if bits_per_sample.is_none() {
        bits_per_sample = codec_bits_per_sample(decoder.codec_params());
    }

    Ok((
        DecodedAudio {
            sample_rate,
            channels,
            peaks,
        },
        DecodeMeta {
            container_format,
            codec,
            bits_per_sample,
        },
    ))
}

fn codec_bits_per_sample(params: &CodecParameters) -> Option<u32> {
    params
        .bits_per_sample
        .or(params.bits_per_coded_sample)
        .or_else(|| {
            params.sample_format.map(|format| match format {
                SampleFormat::U8 | SampleFormat::S8 => 8,
                SampleFormat::U16 | SampleFormat::S16 => 16,
                SampleFormat::U24 | SampleFormat::S24 => 24,
                SampleFormat::U32 | SampleFormat::S32 | SampleFormat::F32 => 32,
                SampleFormat::F64 => 64,
            })
        })
}

impl WaveformDataProvider for Buffer {
    fn sample_rate(&self) -> u32 {
        self.audio.sample_rate
    }

    fn channel_count(&self) -> usize {
        self.audio.channel_count()
    }

    fn frames(&self) -> usize {
        self.audio.frames()
    }

    fn duration_secs(&self) -> f64 {
        self.audio.duration_secs()
    }

    fn channel_label(&self, channel: usize) -> String {
        WaveformDataProvider::channel_label(&self.audio, channel)
    }

    fn channel_samples(&self, channel: usize) -> &[f32] {
        WaveformDataProvider::channel_samples(&self.audio, channel)
    }

    fn channel_peaks(&self, channel: usize) -> &[(f32, f32)] {
        WaveformDataProvider::channel_peaks(&self.audio, channel)
    }
}

fn append_decoded(
    decoded: AudioBufferRef<'_>,
    sample_buf: &mut Option<SampleBuffer<f32>>,
    spec: &mut Option<SignalSpec>,
    channels: &mut Vec<Vec<f32>>,
) -> Result<()> {
    let decoded_spec = *decoded.spec();
    let channel_count = decoded_spec.channels.count();
    if channel_count == 0 {
        bail!("audio track has no channels");
    }

    if spec.is_none() {
        *spec = Some(decoded_spec);
        *channels = vec![Vec::new(); channel_count];
        *sample_buf = Some(SampleBuffer::<f32>::new(
            decoded.capacity() as u64,
            decoded_spec,
        ));
    }

    let buf = sample_buf
        .as_mut()
        .expect("sample buffer is created with the first packet");
    buf.copy_interleaved_ref(decoded);

    let samples = buf.samples();
    if samples.len() % channel_count != 0 {
        bail!("decoded sample count is not a multiple of the channel count");
    }

    for frame in samples.chunks_exact(channel_count) {
        for (ch, sample) in frame.iter().enumerate() {
            channels[ch].push(*sample);
        }
    }

    Ok(())
}

fn build_peaks(samples: &[f32]) -> Vec<(f32, f32)> {
    samples
        .chunks(PEAK_BLOCK)
        .map(|chunk| {
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            for &s in chunk {
                min = min.min(s);
                max = max.max(s);
            }
            if min > max {
                (0.0, 0.0)
            } else {
                (min, max)
            }
        })
        .collect()
}

/// Min/max of `samples` in `[start, end)`, using peak bins when the range is large.
pub fn min_max_in_range(samples: &[f32], peaks: &[(f32, f32)], start: f64, end: f64) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let start_i = start.max(0.0).floor() as usize;
    let end_i = (end.ceil() as usize).clamp(start_i, samples.len());
    if start_i >= end_i {
        return (0.0, 0.0);
    }

    let mut min = f32::MAX;
    let mut max = f32::MIN;

    if end_i - start_i >= PEAK_BLOCK * 2 && !peaks.is_empty() {
        let peak_start = start_i / PEAK_BLOCK;
        let peak_end = ((end_i + PEAK_BLOCK - 1) / PEAK_BLOCK).min(peaks.len());
        for &(pmin, pmax) in &peaks[peak_start..peak_end] {
            min = min.min(pmin);
            max = max.max(pmax);
        }
    } else {
        for &s in &samples[start_i..end_i] {
            min = min.min(s);
            max = max.max(s);
        }
    }

    if min > max {
        (0.0, 0.0)
    } else {
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Write, path::Path};

    fn write_sine_wav(path: &Path, channels: u16, frames: u32, sample_rate: u32) {
        let bits_per_sample: u16 = 16;
        let block_align = channels * bits_per_sample / 8;
        let byte_rate = sample_rate * u32::from(block_align);
        let data_len = frames * u32::from(block_align);
        let mut out = File::create(path).unwrap();
        out.write_all(b"RIFF").unwrap();
        out.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        out.write_all(b"WAVE").unwrap();
        out.write_all(b"fmt ").unwrap();
        out.write_all(&16u32.to_le_bytes()).unwrap();
        out.write_all(&1u16.to_le_bytes()).unwrap();
        out.write_all(&channels.to_le_bytes()).unwrap();
        out.write_all(&sample_rate.to_le_bytes()).unwrap();
        out.write_all(&byte_rate.to_le_bytes()).unwrap();
        out.write_all(&block_align.to_le_bytes()).unwrap();
        out.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        out.write_all(b"data").unwrap();
        out.write_all(&data_len.to_le_bytes()).unwrap();
        for i in 0..frames {
            let t = i as f32 / sample_rate as f32;
            for ch in 0..channels {
                let freq = 440.0 * (ch as f32 + 1.0);
                let sample = (t * freq * std::f32::consts::TAU).sin();
                let pcm = (sample * 0.6 * i16::MAX as f32) as i16;
                out.write_all(&pcm.to_le_bytes()).unwrap();
            }
        }
    }

    #[test]
    fn peaks_cover_all_samples() {
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32) / 1000.0 - 0.5).collect();
        let peaks = build_peaks(&samples);
        assert_eq!(peaks.len(), (1000 + PEAK_BLOCK - 1) / PEAK_BLOCK);
        let (min, max) = min_max_in_range(&samples, &peaks, 0.0, 1000.0);
        assert!(min < 0.0);
        assert!(max > 0.0);
    }

    #[test]
    fn decodes_stereo_wav() {
        let dir = std::env::temp_dir();
        let path = dir.join("snd-display-stereo-test.wav");
        write_sine_wav(&path, 2, 4410, 44100);
        let audio = decode(&path).expect("decode stereo wav");
        assert_eq!(audio.channel_count(), 2);
        assert_eq!(audio.sample_rate, 44100);
        assert_eq!(audio.frames(), 4410);
        assert_eq!(audio.channels[0].len(), audio.channels[1].len());
        assert!(audio.channels[0].iter().any(|s| s.abs() > 0.1));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_buffer_captures_wav_source_meta() {
        let dir = std::env::temp_dir();
        let path = dir.join("snd-display-source-meta-test.wav");
        write_sine_wav(&path, 2, 4410, 44100);
        let buffer = load_buffer(&path).expect("load wav");
        let source = buffer.source.expect("source metadata");
        assert_eq!(source.bits_per_sample, Some(16));
        assert_eq!(source.size_bytes, std::fs::metadata(&path).unwrap().len());
        let _ = std::fs::remove_file(path);
    }
}
