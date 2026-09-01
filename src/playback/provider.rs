// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::sync::{Arc, RwLock};

use crate::audio::DecodedAudio;
use crate::model::composition::Composition;
use crate::model::Buffer;

pub trait PlaybackDataProvider: Send + Sync {
    fn sample_rate(&self) -> u32;
    fn channel_count(&self) -> usize;
    fn frames(&self) -> usize;
    fn read_interleaved(&self, start: usize, count: usize, dest: &mut [f32]);
}

/// Playback provider whose composition target can be swapped without replacing
/// the `Arc<dyn PlaybackDataProvider>` the engine already holds.
pub struct SharedCompositionProvider {
    current: RwLock<Arc<RwLock<Composition>>>,
}

impl SharedCompositionProvider {
    pub fn new(composition: Arc<RwLock<Composition>>) -> Self {
        Self {
            current: RwLock::new(composition),
        }
    }

    pub fn bind(&self, composition: Arc<RwLock<Composition>>) {
        *self.current.write().unwrap() = composition;
    }

    fn composition(&self) -> Arc<RwLock<Composition>> {
        self.current.read().unwrap().clone()
    }
}

impl PlaybackDataProvider for SharedCompositionProvider {
    fn sample_rate(&self) -> u32 {
        self.composition().read().unwrap().sample_rate()
    }

    fn channel_count(&self) -> usize {
        self.composition().read().unwrap().channel_count()
    }

    fn frames(&self) -> usize {
        self.composition().read().unwrap().frames() as usize
    }

    fn read_interleaved(&self, start: usize, count: usize, dest: &mut [f32]) {
        let _ =
            self.composition()
                .read()
                .unwrap()
                .read_interleaved(start as u64, count as u64, dest);
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
    use crate::model::MediaRef;

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

    fn memory_composition(frames: usize) -> Composition {
        use crate::model::composition::MediaId;
        let samples = vec![vec![0.0; frames]];
        let media = MediaRef::from_memory(MediaId(0), 44100, samples);
        Composition::from_media(media).unwrap()
    }

    #[test]
    fn bind_switches_provider_frames() {
        let first = Arc::new(RwLock::new(memory_composition(8)));
        let second = Arc::new(RwLock::new(memory_composition(32)));
        let provider = SharedCompositionProvider::new(first);
        assert_eq!(provider.frames(), 8);
        provider.bind(second);
        assert_eq!(provider.frames(), 32);
    }

    #[test]
    fn bind_leaves_previous_composition_intact() {
        let first = Arc::new(RwLock::new(memory_composition(8)));
        let second = Arc::new(RwLock::new(memory_composition(32)));
        let provider = SharedCompositionProvider::new(first.clone());
        provider.bind(second);
        assert_eq!(first.read().unwrap().frames(), 8);
        assert_eq!(provider.frames(), 32);
    }
}
