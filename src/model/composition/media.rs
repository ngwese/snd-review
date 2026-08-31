// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: MediaId,
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channel_count: usize,
    pub frame_count: u64,
    pub bits_per_sample: Option<u32>,
    pub size_bytes: u64,
    pub hash: Option<String>,
    #[serde(skip)]
    pub samples: Option<Arc<Vec<Vec<f32>>>>,
}

impl MediaRef {
    pub fn from_memory(id: MediaId, sample_rate: u32, samples: Vec<Vec<f32>>) -> Self {
        let frame_count = samples.first().map(|ch| ch.len() as u64).unwrap_or(0);
        let channel_count = samples.len();
        Self {
            id,
            path: PathBuf::from(format!("memory://{id:?}")),
            sample_rate,
            channel_count,
            frame_count,
            bits_per_sample: Some(32),
            size_bytes: 0,
            hash: None,
            samples: Some(Arc::new(samples)),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaPool {
    next_id: u64,
    media: HashMap<u64, MediaRef>,
}

impl MediaPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, mut media: MediaRef) -> MediaId {
        if media.id.0 == 0 {
            self.next_id += 1;
            media.id = MediaId(self.next_id);
        } else {
            self.next_id = self.next_id.max(media.id.0);
        }
        let id = media.id;
        self.media.insert(id.0, media);
        id
    }

    pub fn get(&self, id: MediaId) -> Option<&MediaRef> {
        self.media.get(&id.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = &MediaRef> {
        self.media.values()
    }

    pub fn first(&self) -> Option<&MediaRef> {
        let min_id = self.media.keys().copied().min()?;
        self.media.get(&min_id)
    }

    pub fn is_empty(&self) -> bool {
        self.media.is_empty()
    }

    pub fn into_refs(self) -> Vec<MediaRef> {
        let mut refs: Vec<_> = self.media.into_values().collect();
        refs.sort_by_key(|m| m.id.0);
        refs
    }
}
