// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use super::buffer::{ChannelScope, RegionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamplePosition {
    pub sample: usize,
    pub channels: ChannelScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Position(SamplePosition),
    Region {
        region_id: Option<RegionId>,
        start: usize,
        end: usize,
        channels: ChannelScope,
    },
}

impl Selection {
    pub fn position_sample(&self) -> Option<usize> {
        match self {
            Selection::Position(pos) => Some(pos.sample),
            _ => None,
        }
    }
}
