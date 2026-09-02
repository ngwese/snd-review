// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

/// PCM sample formats matching Symphonia's `SampleFormat` variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PcmFormat {
    U8,
    S8,
    U16,
    S16,
    U24,
    S24,
    U32,
    S32,
    F32,
    F64,
}

impl PcmFormat {
    pub const ALL: [PcmFormat; 10] = [
        PcmFormat::U8,
        PcmFormat::S8,
        PcmFormat::U16,
        PcmFormat::S16,
        PcmFormat::U24,
        PcmFormat::S24,
        PcmFormat::U32,
        PcmFormat::S32,
        PcmFormat::F32,
        PcmFormat::F64,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PcmFormat::U8 => "U8",
            PcmFormat::S8 => "S8",
            PcmFormat::U16 => "U16",
            PcmFormat::S16 => "S16",
            PcmFormat::U24 => "U24",
            PcmFormat::S24 => "S24",
            PcmFormat::U32 => "U32",
            PcmFormat::S32 => "S32",
            PcmFormat::F32 => "F32",
            PcmFormat::F64 => "F64",
        }
    }

    pub fn bits(self) -> u16 {
        match self {
            PcmFormat::U8 | PcmFormat::S8 => 8,
            PcmFormat::U16 | PcmFormat::S16 => 16,
            PcmFormat::U24 | PcmFormat::S24 => 24,
            PcmFormat::U32 | PcmFormat::S32 | PcmFormat::F32 => 32,
            PcmFormat::F64 => 64,
        }
    }

    pub fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            8 => Some(PcmFormat::U8),
            16 => Some(PcmFormat::S16),
            24 => Some(PcmFormat::S24),
            32 => Some(PcmFormat::F32),
            64 => Some(PcmFormat::F64),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EncoderCaps {
    /// `None` means the encoder does not store a PCM format (lossy).
    pub sample_formats: Option<&'static [PcmFormat]>,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub min_channels: u16,
    pub max_channels: u16,
}

impl EncoderCaps {
    pub fn stores_sample_format(self) -> bool {
        self.sample_formats.is_some()
    }

    pub fn allows_rate(self, rate: u32) -> bool {
        rate > 0 && rate >= self.min_sample_rate && rate <= self.max_sample_rate
    }

    pub fn allows_channels(self, count: u16) -> bool {
        count >= self.min_channels && count <= self.max_channels
    }

    pub fn allows_format(self, format: PcmFormat) -> bool {
        match self.sample_formats {
            None => true,
            Some(list) => list.contains(&format),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EncodeSpec {
    pub sample_rate: u32,
    pub sample_format: Option<PcmFormat>,
    pub channel_count: u16,
}

pub fn snap_format(caps: EncoderCaps, current: Option<PcmFormat>) -> Option<PcmFormat> {
    if !caps.stores_sample_format() {
        return current;
    }
    if let Some(format) = current {
        if caps.allows_format(format) {
            return Some(format);
        }
    }
    const PREFERRED: [PcmFormat; 5] = [
        PcmFormat::S24,
        PcmFormat::S16,
        PcmFormat::F32,
        PcmFormat::S32,
        PcmFormat::U8,
    ];
    PREFERRED
        .into_iter()
        .find(|format| caps.allows_format(*format))
        .or_else(|| caps.sample_formats.and_then(|list| list.first().copied()))
}

pub fn spec_supported(caps: EncoderCaps, spec: &EncodeSpec) -> bool {
    if !caps.allows_rate(spec.sample_rate) || !caps.allows_channels(spec.channel_count) {
        return false;
    }
    match spec.sample_format {
        Some(format) if caps.stores_sample_format() => caps.allows_format(format),
        Some(_) => true,
        None => !caps.stores_sample_format(),
    }
}

pub const RATE_PRESETS: [u32; 8] = [
    22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000,
];

pub fn format_rate(rate: u32) -> String {
    if rate % 1000 == 0 {
        format!("{} kHz", rate / 1000)
    } else if rate % 100 == 0 {
        format!("{:.1} kHz", rate as f64 / 1000.0)
    } else {
        format!("{rate} Hz")
    }
}
