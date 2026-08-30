// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::sync::{Arc, RwLock};

use crate::audio::DecodedAudio;
use crate::model::Buffer;

pub trait PlaybackDataProvider: Send + Sync {
    fn sample_rate(&self) -> u32;
    fn channel_count(&self) -> usize;
    fn frames(&self) -> usize;
    fn read_interleaved(&self, start: usize, count: usize, dest: &mut [f32]);
}

pub struct SharedBufferProvider(pub Arc<RwLock<Buffer>>);

impl PlaybackDataProvider for SharedBufferProvider {
    fn sample_rate(&self) -> u32 {
        self.0.read().unwrap().audio.sample_rate
    }

    fn channel_count(&self) -> usize {
        self.0.read().unwrap().audio.channel_count()
    }

    fn frames(&self) -> usize {
        self.0.read().unwrap().frames()
    }

    fn read_interleaved(&self, start: usize, count: usize, dest: &mut [f32]) {
        let buffer = self.0.read().unwrap();
        buffer.read_interleaved(start, count, dest);
    }
}

impl PlaybackDataProvider for DecodedAudio {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channel_count(&self) -> usize {
        self.channel_count()
    }

    fn frames(&self) -> usize {
        self.frames()
    }

    fn read_interleaved(&self, start: usize, count: usize, dest: &mut [f32]) {
        read_planar_interleaved(&self.channels, start, count, dest);
    }
}

impl PlaybackDataProvider for Buffer {
    fn sample_rate(&self) -> u32 {
        self.audio.sample_rate
    }

    fn channel_count(&self) -> usize {
        self.audio.channel_count()
    }

    fn frames(&self) -> usize {
        self.frames()
    }

    fn read_interleaved(&self, start: usize, count: usize, dest: &mut [f32]) {
        read_planar_interleaved(&self.audio.channels, start, count, dest);
    }
}

fn read_planar_interleaved(channels: &[Vec<f32>], start: usize, count: usize, dest: &mut [f32]) {
    let ch_count = channels.len();
    if ch_count == 0 {
        dest.fill(0.0);
        return;
    }
    let total = count * ch_count;
    debug_assert!(dest.len() >= total);
    dest[..total].fill(0.0);

    for (ch, samples) in channels.iter().enumerate() {
        let end = (start + count).min(samples.len());
        if start >= end {
            continue;
        }
        let slice = &samples[start..end];
        for (frame, &sample) in slice.iter().enumerate() {
            dest[frame * ch_count + ch] = sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_stereo_frames() {
        let audio = DecodedAudio {
            sample_rate: 44100,
            channels: vec![vec![1.0, 2.0], vec![3.0, 4.0]],
            peaks: vec![vec![], vec![]],
        };
        let mut dest = [0.0; 4];
        audio.read_interleaved(0, 2, &mut dest);
        assert_eq!(dest, [1.0, 3.0, 2.0, 4.0]);
    }

    #[test]
    fn zero_fills_past_end() {
        let audio = DecodedAudio {
            sample_rate: 44100,
            channels: vec![vec![1.0], vec![2.0]],
            peaks: vec![vec![], vec![]],
        };
        let mut dest = [9.0; 4];
        audio.read_interleaved(1, 2, &mut dest);
        assert_eq!(dest, [0.0, 0.0, 0.0, 0.0]);
    }
}
