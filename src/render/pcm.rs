// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use super::spec::PcmFormat;

pub fn clamp_unit(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

pub fn to_signed(sample: f32, bits: u32) -> i32 {
    let max = (1i32 << (bits - 1)) - 1;
    (clamp_unit(sample) * max as f32).round() as i32
}

pub fn to_u8(sample: f32) -> u8 {
    ((clamp_unit(sample) * 0.5 + 0.5) * 255.0).round() as u8
}

pub fn planar_frames(planar: &[Vec<f32>]) -> usize {
    planar.iter().map(|ch| ch.len()).min().unwrap_or(0)
}

pub fn interleave_f32(planar: &[Vec<f32>]) -> Vec<f32> {
    let frames = planar_frames(planar);
    let channels = planar.len();
    let mut out = vec![0.0; frames * channels];
    for frame in 0..frames {
        for (ch, plane) in planar.iter().enumerate() {
            out[frame * channels + ch] = plane[frame];
        }
    }
    out
}

pub fn interleave_i32(planar: &[Vec<f32>], bits: u32) -> Vec<i32> {
    let frames = planar_frames(planar);
    let channels = planar.len();
    let mut out = vec![0; frames * channels];
    for frame in 0..frames {
        for (ch, plane) in planar.iter().enumerate() {
            out[frame * channels + ch] = to_signed(plane[frame], bits);
        }
    }
    out
}

pub fn select_channels(planar: &[Vec<f32>], indices: &[usize]) -> Vec<Vec<f32>> {
    indices
        .iter()
        .map(|index| planar.get(*index).cloned().unwrap_or_default())
        .collect()
}

pub fn bits_for_integer(format: PcmFormat) -> Option<u32> {
    match format {
        PcmFormat::S8 => Some(8),
        PcmFormat::S16 => Some(16),
        PcmFormat::S24 => Some(24),
        PcmFormat::S32 => Some(32),
        _ => None,
    }
}
