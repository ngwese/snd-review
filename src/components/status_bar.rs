// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use gpui::{px, App, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window};
use gpui_component::status_bar::StatusBar;

use crate::model::composition::Composition;
use crate::model::Buffer;

const HEIGHT: gpui::Pixels = px(24.);

pub struct FileStatus {
    pub sample_rate: u32,
    pub bits_per_sample: Option<u32>,
    pub channel_count: usize,
    pub duration_secs: f64,
    pub size_bytes: Option<u64>,
}

impl FileStatus {
    pub fn from_composition(composition: &Composition) -> Option<Self> {
        if composition.frames() == 0 {
            return None;
        }
        let media = composition.pool().first();
        Some(Self {
            sample_rate: composition.sample_rate(),
            bits_per_sample: media.and_then(|m| m.bits_per_sample),
            channel_count: composition.channel_count(),
            duration_secs: composition.duration_secs(),
            size_bytes: media.map(|m| m.size_bytes),
        })
    }

    pub fn from_buffer(buffer: &Buffer) -> Option<Self> {
        if !buffer.is_loaded() {
            return None;
        }
        Some(Self {
            sample_rate: buffer.audio.sample_rate,
            bits_per_sample: buffer
                .source
                .as_ref()
                .and_then(|source| source.bits_per_sample),
            channel_count: buffer.audio.channel_count(),
            duration_secs: buffer.audio.duration_secs(),
            size_bytes: buffer.source.as_ref().map(|source| source.size_bytes),
        })
    }
}

#[derive(IntoElement)]
pub struct FileStatusBar {
    file: Option<FileStatus>,
    progress_message: Option<String>,
}

impl FileStatusBar {
    pub fn new(file: Option<FileStatus>) -> Self {
        Self {
            file,
            progress_message: None,
        }
    }

    pub fn with_progress_message(mut self, message: Option<String>) -> Self {
        self.progress_message = message;
        self
    }
}

impl RenderOnce for FileStatusBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut bar = StatusBar::new()
            .w_full()
            .flex_none()
            .h(HEIGHT)
            .min_h(HEIGHT)
            .max_h(HEIGHT)
            .text_xs();
        let has_file = self.file.is_some();
        if let Some(file) = self.file {
            bar = bar
                .left(format!("{} Hz", file.sample_rate))
                .left(format_bit_depth(file.bits_per_sample))
                .left(format!("{} ch", file.channel_count))
                .left(format_duration(file.duration_secs))
                .left(
                    file.size_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "—".into()),
                );
        }
        if let Some(message) = self.progress_message {
            if !has_file {
                bar = bar.left("");
            }
            bar = bar.child(message).right("");
        }
        bar
    }
}

fn format_bit_depth(bits: Option<u32>) -> String {
    match bits {
        Some(bits) => format!("{bits}-bit"),
        None => "—".into(),
    }
}

fn format_duration(secs: f64) -> String {
    if secs.is_nan() || secs.is_infinite() {
        return "0.00s".into();
    }
    if secs < 60.0 {
        format!("{secs:.2}s")
    } else {
        let m = (secs / 60.0).floor() as u32;
        let s = secs % 60.0;
        format!("{m}m {s:05.2}s")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_byte_sizes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_572_864), "1.5 MB");
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(0.0), "0.00s");
        assert_eq!(format_duration(1.5), "1.50s");
        assert_eq!(format_duration(61.5), "1m 01.50s");
    }

    #[test]
    fn formats_bit_depth() {
        assert_eq!(format_bit_depth(Some(16)), "16-bit");
        assert_eq!(format_bit_depth(None), "—");
    }
}
