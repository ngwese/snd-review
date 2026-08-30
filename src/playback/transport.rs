// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
}

impl TransportState {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Playing,
            2 => Self::Paused,
            _ => Self::Stopped,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Stopped => 0,
            Self::Playing => 1,
            Self::Paused => 2,
        }
    }
}

pub struct Transport {
    state: TransportState,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            state: TransportState::Stopped,
        }
    }

    pub fn state(&self) -> TransportState {
        self.state
    }

    pub fn set_state(&mut self, state: TransportState) {
        self.state = state;
    }

    pub fn is_playing(&self) -> bool {
        self.state == TransportState::Playing
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}
