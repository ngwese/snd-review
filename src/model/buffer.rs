use std::{path::PathBuf, time::SystemTime};

use crate::audio::DecodedAudio;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarkerId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelScope {
    AllChannels,
    Channels(Vec<usize>),
}

impl ChannelScope {
    pub fn all() -> Self {
        Self::AllChannels
    }

    pub fn single(channel: usize) -> Self {
        Self::Channels(vec![channel])
    }

    pub fn applies_to(&self, channel: usize) -> bool {
        match self {
            Self::AllChannels => true,
            Self::Channels(channels) => channels.contains(&channel),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BufferSource {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size_bytes: u64,
    pub container_format: String,
    pub codec: String,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub id: RegionId,
    pub start: usize,
    pub end: usize,
    pub channels: ChannelScope,
}

impl Region {
    pub fn new(id: RegionId, start: usize, end: usize, channels: ChannelScope) -> Self {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self {
            id,
            start,
            end,
            channels,
        }
    }

    pub fn contains(&self, sample: usize, channel: usize) -> bool {
        self.channels.applies_to(channel) && sample >= self.start && sample <= self.end
    }
}

#[derive(Debug, Clone)]
pub struct Marker {
    pub id: MarkerId,
    pub sample: usize,
    pub channels: ChannelScope,
    pub color: Option<[f32; 4]>,
    pub label_type: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug)]
pub struct Buffer {
    pub audio: DecodedAudio,
    pub source: Option<BufferSource>,
    pub regions: Vec<Region>,
    pub markers: Vec<Marker>,
}

impl Buffer {
    pub fn frames(&self) -> usize {
        self.audio.frames()
    }

    pub fn full_region(&self) -> Region {
        let end = self.frames().saturating_sub(1);
        Region {
            id: RegionId(0),
            start: 0,
            end,
            channels: ChannelScope::all(),
        }
    }

    pub fn region(&self, id: RegionId) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    pub fn region_mut(&mut self, id: RegionId) -> Option<&mut Region> {
        self.regions.iter_mut().find(|r| r.id == id)
    }
}
