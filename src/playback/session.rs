use std::sync::{Arc, RwLock};

use anyhow::Result;
use cpal::Device;

use crate::model::buffer::ChannelScope;
use crate::model::document::BufferDocument;
use crate::model::selection::Selection;
use crate::model::Buffer;

use super::anchors::{collect_anchors, next_anchor, previous_anchor};
use super::engine::PlaybackEngine;
use super::playhead::Playhead;
use super::provider::{PlaybackDataProvider, SharedBufferProvider};
use super::transport::{Transport, TransportState};

pub struct PlaybackSession {
    playhead: Playhead,
    transport: Transport,
    engine: PlaybackEngine,
    anchors: Vec<usize>,
    active_region: Option<(usize, usize)>,
}

impl PlaybackSession {
    pub fn open(device: &Device, buffer: Arc<RwLock<Buffer>>) -> Result<Self> {
        let anchors = collect_anchors(&buffer.read().unwrap());
        let provider: Arc<dyn PlaybackDataProvider> =
            Arc::new(SharedBufferProvider(buffer));
        let playhead = Playhead::new(provider.clone());
        let engine = PlaybackEngine::open(device, provider)?;
        Ok(Self {
            playhead,
            transport: Transport::new(),
            engine,
            anchors,
            active_region: None,
        })
    }

    pub fn transport_state(&self) -> TransportState {
        self.transport.state()
    }

    pub fn looping(&self) -> bool {
        self.playhead.looping()
    }

    pub fn position(&self) -> usize {
        if self.transport.is_playing() {
            self.engine.shared.position()
        } else {
            self.playhead.position()
        }
    }

    pub fn refresh_anchors(&mut self, buffer: &Buffer) {
        self.anchors = collect_anchors(buffer);
    }

    fn refresh_anchors_from_doc(&mut self, doc: &BufferDocument) {
        self.refresh_anchors(&doc.buffer.read().unwrap());
    }

    fn apply_to_engine(&self) {
        self.engine.shared.set_position(self.playhead.position());
        self.engine
            .shared
            .set_looping(self.playhead.looping());
        self.engine.shared.set_in_out(
            self.playhead.in_point(),
            self.playhead.out_point(),
        );
        self.engine
            .shared
            .set_transport(self.transport.state());
    }

    fn sync_playhead_from_engine(&mut self) {
        if self.transport.is_playing() {
            self.playhead
                .set_position(self.engine.shared.position());
        }
    }

    fn region_bounds_from_doc(doc: &BufferDocument) -> Option<(usize, usize)> {
        match &doc.selection {
            Selection::Region { start, end, .. } if end > start => Some((*start, *end)),
            _ => None,
        }
    }

    pub fn sync_from_document(&mut self, doc: &BufferDocument) {
        self.refresh_anchors_from_doc(doc);

        if doc.is_region_drag_active() {
            if !self.transport.is_playing() {
                if let Some((start, end)) = self.active_region {
                    self.playhead.set_in_out(start, end);
                }
                if let Some(pos) = &doc.current_position {
                    self.playhead.set_position(pos.sample);
                }
                self.apply_to_engine();
            }
            return;
        }

        let new_region = Self::region_bounds_from_doc(doc);
        let region_changed = new_region != self.active_region;
        self.active_region = new_region;

        if let Some((start, end)) = new_region {
            self.playhead.set_in_out(start, end);
            if self.transport.is_playing() {
                if region_changed {
                    self.playhead.set_position(start);
                }
            } else if let Some(pos) = &doc.current_position {
                self.playhead.set_position(pos.sample);
            }
        } else {
            self.playhead.clear_in_out();
            if self.transport.is_playing() {
                if let Some(pos) = &doc.current_position {
                    self.playhead.set_position(pos.sample);
                }
            } else if let Some(pos) = &doc.current_position {
                self.playhead.set_position(pos.sample);
            }
        }
        self.apply_to_engine();
    }

    pub fn sync_document_from_playback(&mut self, doc: &mut BufferDocument) {
        self.sync_playhead_from_engine();
        let sample = self.playhead.position();
        doc.set_position_from_playback(sample, ChannelScope::all());
        self.apply_to_engine();
    }

    pub fn start(&mut self) {
        self.playhead.set_position(self.playhead.playback_start());
        self.transport.set_state(TransportState::Playing);
        self.apply_to_engine();
    }

    pub fn play(&mut self) {
        match self.transport.state() {
            TransportState::Paused => {
                self.transport.set_state(TransportState::Playing);
            }
            TransportState::Stopped => {
                if self.playhead.in_point().is_some() {
                    self.playhead.set_position(self.playhead.playback_start());
                }
                self.transport.set_state(TransportState::Playing);
            }
            TransportState::Playing => {}
        }
        self.apply_to_engine();
    }

    pub fn pause(&mut self) {
        self.sync_playhead_from_engine();
        self.transport.set_state(TransportState::Paused);
        self.apply_to_engine();
    }

    pub fn stop(&mut self) {
        self.sync_playhead_from_engine();
        self.transport.set_state(TransportState::Stopped);
        self.apply_to_engine();
    }

    pub fn home(&mut self) {
        self.playhead.set_position(self.playhead.playback_start());
        self.transport.set_state(TransportState::Stopped);
        self.apply_to_engine();
    }

    pub fn end(&mut self) {
        self.playhead.set_position(self.playhead.transport_end());
        self.transport.set_state(TransportState::Stopped);
        self.apply_to_engine();
    }

    pub fn previous(&mut self) {
        let pos = self.playhead.position();
        if let Some(anchor) = previous_anchor(&self.anchors, pos) {
            self.playhead.set_position(anchor);
        }
        if self.transport.state() == TransportState::Playing {
            self.transport.set_state(TransportState::Stopped);
        }
        self.apply_to_engine();
    }

    pub fn next(&mut self) {
        let pos = self.playhead.position();
        if let Some(anchor) = next_anchor(&self.anchors, pos) {
            self.playhead.set_position(anchor);
        }
        if self.transport.state() == TransportState::Playing {
            self.transport.set_state(TransportState::Stopped);
        }
        self.apply_to_engine();
    }

    pub fn toggle_loop(&mut self) {
        self.playhead.toggle_looping();
        self.apply_to_engine();
    }

    pub fn toggle_play_pause(&mut self) {
        match self.transport.state() {
            TransportState::Playing => self.pause(),
            TransportState::Paused | TransportState::Stopped => self.play(),
        }
    }

    pub fn poll(&mut self, doc: &mut BufferDocument) {
        let engine_state = self.engine.shared.transport();
        if engine_state == TransportState::Stopped
            && self.transport.state() == TransportState::Playing
        {
            self.transport.set_state(TransportState::Stopped);
            self.sync_playhead_from_engine();
        }
        if self.transport.is_playing() {
            self.sync_document_from_playback(doc);
        }
    }
}
