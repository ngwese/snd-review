// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::sync::{Arc, RwLock};

use super::buffer::{Buffer, ChannelScope, MarkerId, Region, RegionId};
use super::composition::{Composition, EditId};
use super::selection::{SamplePosition, Selection};
use super::snap::nearest_zero_crossing;
use crate::components::waveform::WaveformDataProvider;

const DRAG_THRESHOLD_SAMPLES: usize = 0;

pub struct BufferDocument {
    pub composition: Arc<RwLock<Composition>>,
    pub buffer: Arc<RwLock<Buffer>>,
    pub selection: Selection,
    pub current_position: Option<SamplePosition>,
    pub snap_zero_crossings: bool,
    region_drag_anchor: Option<usize>,
    next_region_id: u64,
    next_marker_id: u64,
}

impl BufferDocument {
    pub fn new(composition: Composition) -> Self {
        Self::with_shared(
            Arc::new(RwLock::new(composition)),
            Arc::new(RwLock::new(Buffer::empty())),
        )
    }

    pub fn with_shared(composition: Arc<RwLock<Composition>>, buffer: Arc<RwLock<Buffer>>) -> Self {
        Self {
            composition,
            buffer,
            selection: Selection::None,
            current_position: None,
            snap_zero_crossings: false,
            region_drag_anchor: None,
            next_region_id: 1,
            next_marker_id: 1,
        }
    }

    pub fn frames(&self) -> usize {
        self.composition.read().unwrap().frames() as usize
    }

    pub fn sample_rate(&self) -> u32 {
        self.composition.read().unwrap().sample_rate()
    }

    pub fn is_loaded(&self) -> bool {
        self.frames() > 0
    }

    pub fn toggle_zero_crossing_snap(&mut self) {
        self.snap_zero_crossings = !self.snap_zero_crossings;
    }

    pub fn is_region_drag_active(&self) -> bool {
        self.region_drag_anchor.is_some()
    }

    fn snap_channel(&self, channel: usize, sample: usize) -> usize {
        if !self.snap_zero_crossings {
            return sample;
        }
        let frames = self.frames();
        if frames == 0 {
            return 0;
        }
        let radius = 4096;
        let start = sample.saturating_sub(radius);
        let end = (sample + radius + 1).min(frames);
        let mut buf = vec![0.0; end.saturating_sub(start)];
        let _ = self
            .composition
            .read()
            .unwrap()
            .read_channel(channel, start as u64, &mut buf);
        let local = nearest_zero_crossing(&buf, sample.saturating_sub(start), radius);
        start + local
    }

    fn snap_sample(&self, scope: &ChannelScope, sample: usize) -> usize {
        if !self.snap_zero_crossings {
            return sample;
        }
        let channel = match scope {
            ChannelScope::AllChannels => 0,
            ChannelScope::Channels(chs) => chs.first().copied().unwrap_or(0),
        };
        self.snap_channel(channel, sample)
    }

    fn clamp_sample(&self, sample: usize) -> usize {
        let max = self.frames().saturating_sub(1);
        sample.min(max)
    }

    pub fn sample_to_secs(&self, sample: usize) -> f64 {
        let rate = self.sample_rate();
        if rate == 0 {
            0.0
        } else {
            sample as f64 / f64::from(rate)
        }
    }

    pub fn normalized_region_bounds(start: usize, end: usize) -> (usize, usize) {
        if start <= end {
            (start, end)
        } else {
            (end, start)
        }
    }

    fn set_current_position_sample(&mut self, sample: usize, channels: ChannelScope) {
        self.current_position = Some(SamplePosition { sample, channels });
    }

    pub fn channel_scope_for_lane(&self, lane: usize, alt: bool) -> ChannelScope {
        if alt {
            ChannelScope::single(lane)
        } else {
            ChannelScope::all()
        }
    }

    pub fn hit_test_region(&self, sample: usize, lane: usize) -> Option<RegionId> {
        self.buffer
            .read()
            .unwrap()
            .regions
            .iter()
            .rev()
            .find(|region| region.contains(sample, lane))
            .map(|region| region.id)
    }

    pub fn set_position(&mut self, sample: usize, scope: ChannelScope) {
        let sample = self.clamp_sample(self.snap_sample(&scope, sample));
        self.region_drag_anchor = None;
        self.set_current_position_sample(sample, scope.clone());
        self.selection = Selection::Position(SamplePosition {
            sample,
            channels: scope,
        });
    }

    pub fn select_region_at(&mut self, sample: usize, lane: usize, scope: ChannelScope) {
        let sample = self.clamp_sample(self.snap_sample(&scope, sample));
        if let Some(id) = self.hit_test_region(sample, lane) {
            let region = self.buffer.read().unwrap().region(id).cloned();
            if let Some(region) = region {
                self.region_drag_anchor = None;
                self.set_current_position_sample(region.end, region.channels.clone());
                self.selection = Selection::Region {
                    region_id: Some(id),
                    start: region.start,
                    end: region.end,
                    channels: region.channels,
                };
                return;
            }
        }
        self.set_position(sample, scope);
    }

    pub fn begin_region_drag(&mut self, anchor: usize, scope: ChannelScope) {
        let anchor = self.clamp_sample(self.snap_sample(&scope, anchor));
        self.region_drag_anchor = Some(anchor);
        self.selection = Selection::Region {
            region_id: None,
            start: anchor,
            end: anchor,
            channels: scope,
        };
    }

    pub fn update_region_drag(&mut self, sample: usize) {
        let Some(anchor) = self.region_drag_anchor else {
            return;
        };
        let channels = match &self.selection {
            Selection::Region { channels, .. } => channels.clone(),
            _ => return,
        };
        let sample = self.clamp_sample(self.snap_sample(&channels, sample));
        let (start, end) = Self::normalized_region_bounds(anchor, sample);
        let start = self.clamp_sample(self.snap_sample(&channels, start));
        let end = self.clamp_sample(self.snap_sample(&channels, end));
        let (start, end) = Self::normalized_region_bounds(start, end);
        if let Selection::Region {
            start: sel_start,
            end: sel_end,
            ..
        } = &mut self.selection
        {
            *sel_start = start;
            *sel_end = end;
        }
    }

    pub fn finish_region_drag(&mut self) {
        self.region_drag_anchor = None;
        let Selection::Region {
            region_id,
            start,
            end,
            channels,
        } = self.selection.clone()
        else {
            return;
        };

        let (start, end) = Self::normalized_region_bounds(start, end);

        if end.saturating_sub(start) <= DRAG_THRESHOLD_SAMPLES {
            self.set_current_position_sample(start, channels.clone());
            self.selection = Selection::Position(SamplePosition {
                sample: start,
                channels,
            });
            return;
        }

        self.set_current_position_sample(end, channels.clone());

        if let Some(id) = region_id {
            if let Some(region) = self.buffer.write().unwrap().region_mut(id) {
                region.start = start;
                region.end = end;
                region.channels = channels.clone();
            }
            self.selection = Selection::Region {
                region_id: Some(id),
                start,
                end,
                channels,
            };
        } else {
            self.selection = Selection::Region {
                region_id: None,
                start,
                end,
                channels,
            };
        }
    }

    pub fn add_region(&mut self, start: usize, end: usize, channels: ChannelScope) -> RegionId {
        let id = RegionId(self.next_region_id);
        self.next_region_id += 1;
        let region = Region::new(id, start, end, channels);
        self.buffer.write().unwrap().regions.push(region);
        id
    }

    pub fn add_marker(&mut self, sample: usize, channels: ChannelScope) -> MarkerId {
        let id = MarkerId(self.next_marker_id);
        self.next_marker_id += 1;
        self.buffer
            .write()
            .unwrap()
            .markers
            .push(super::buffer::Marker {
                id,
                sample,
                channels,
                color: None,
                label_type: None,
                message: None,
            });
        id
    }

    pub fn selection_position_sample(&self) -> Option<usize> {
        self.current_position.as_ref().map(|pos| pos.sample)
    }

    pub fn set_position_from_playback(&mut self, sample: usize, scope: ChannelScope) {
        if self.is_region_drag_active() {
            return;
        }
        let sample = self.clamp_sample(sample);
        if self
            .current_position
            .as_ref()
            .is_some_and(|pos| pos.sample == sample)
        {
            return;
        }
        self.set_current_position_sample(sample, scope);
    }

    pub fn reset_for_new_buffer(&mut self) {
        self.selection = Selection::None;
        self.current_position = None;
        self.region_drag_anchor = None;
    }

    pub fn selection_span(&self) -> Option<(u64, u64)> {
        match &self.selection {
            Selection::Region { start, end, .. } if *end >= *start => {
                let len = (*end as u64)
                    .saturating_sub(*start as u64)
                    .saturating_add(1);
                Some((*start as u64, len))
            }
            _ => None,
        }
    }

    pub fn caret_frame(&self) -> u64 {
        self.current_position
            .as_ref()
            .map(|pos| pos.sample as u64)
            .unwrap_or(0)
    }

    fn after_edit(&mut self) {
        if self.frames() == 0 {
            self.selection = Selection::None;
            self.current_position = None;
            return;
        }
        if let Some(sample) = self.current_position.as_ref().map(|p| p.sample) {
            let sample = self.clamp_sample(sample);
            if let Some(pos) = self.current_position.as_mut() {
                pos.sample = sample;
            }
        }
        match self.selection.clone() {
            Selection::Region {
                region_id,
                start,
                end,
                channels,
            } => {
                self.selection = Selection::Region {
                    region_id,
                    start: self.clamp_sample(start),
                    end: self.clamp_sample(end),
                    channels,
                };
            }
            Selection::Position(pos) => {
                self.selection = Selection::Position(SamplePosition {
                    sample: self.clamp_sample(pos.sample),
                    channels: pos.channels,
                });
            }
            Selection::None => {}
        }
    }

    pub fn edit_undo(&mut self) -> bool {
        let ok = self.composition.write().unwrap().undo();
        if ok {
            self.after_edit();
        }
        ok
    }

    pub fn edit_redo(&mut self) -> bool {
        let ok = self.composition.write().unwrap().redo();
        if ok {
            self.after_edit();
        }
        ok
    }

    pub fn edit_copy(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        self.composition.write().unwrap().copy(start, len);
    }

    pub fn edit_cut(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        self.composition.write().unwrap().cut(start, len);
        self.after_edit();
    }

    pub fn edit_paste(&mut self) {
        let (at, replace) = if let Some((start, len)) = self.selection_span() {
            (start, len)
        } else {
            (self.caret_frame(), 0)
        };
        let _ = self
            .composition
            .write()
            .unwrap()
            .paste_replacing(at, replace);
        self.after_edit();
    }

    pub fn edit_delete(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        self.composition.write().unwrap().delete(start, len);
        self.after_edit();
    }

    pub fn edit_remove(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        self.composition.write().unwrap().remove(start, len);
        self.after_edit();
    }

    pub fn edit_duplicate(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        self.composition.write().unwrap().duplicate(start, len);
        self.after_edit();
    }

    pub fn edit_trim(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        self.composition.write().unwrap().trim(start, len);
        self.after_edit();
    }

    pub fn edit_roll(&mut self, delta: i64) {
        let at = self
            .selection_span()
            .map(|(start, _)| start)
            .unwrap_or_else(|| self.caret_frame());
        self.composition.write().unwrap().roll(at, delta);
        self.after_edit();
    }

    pub fn current_edit(&self) -> EditId {
        self.composition.read().unwrap().current_edit()
    }
}

impl WaveformDataProvider for BufferDocument {
    fn sample_rate(&self) -> u32 {
        self.composition.read().unwrap().sample_rate()
    }

    fn channel_count(&self) -> usize {
        self.composition.read().unwrap().channel_count()
    }

    fn frames(&self) -> usize {
        self.composition.read().unwrap().frames() as usize
    }

    fn duration_secs(&self) -> f64 {
        self.composition.read().unwrap().duration_secs()
    }

    fn channel_label(&self, channel: usize) -> String {
        match (self.composition.read().unwrap().channel_count(), channel) {
            (1, 0) => "Mono".into(),
            (2, 0) => "L".into(),
            (2, 1) => "R".into(),
            _ => format!("Ch {}", channel + 1),
        }
    }

    fn read_channel(&self, channel: usize, start: usize, dest: &mut [f32]) {
        let _ = self
            .composition
            .read()
            .unwrap()
            .read_channel(channel, start as u64, dest);
    }

    fn min_max_in_range(&self, channel: usize, start: f64, end: f64) -> (f32, f32) {
        self.composition
            .read()
            .unwrap()
            .min_max_in_range(channel, start, end)
    }

    fn fill_minmax_columns(
        &self,
        channel: usize,
        start: f64,
        samples_per_pixel: f64,
        dest: &mut [(f32, f32)],
    ) {
        let composition = self.composition.read().unwrap();
        composition.fill_minmax_columns(channel, start, samples_per_pixel, dest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::composition::{Composition, MediaId, MediaRef};

    fn test_document(frames: usize) -> BufferDocument {
        let samples = vec![vec![0.0; frames], vec![0.0; frames]];
        let media = MediaRef::from_memory(MediaId(0), 44100, samples);
        BufferDocument::new(Composition::from_media(media).unwrap())
    }

    #[test]
    fn full_region_spans_buffer() {
        let doc = test_document(1000);
        assert_eq!(doc.frames(), 1000);
        let region = doc.buffer.read().unwrap().full_region();
        assert_eq!(region.start, 0);
        assert!(region.channels.applies_to(0));
        assert!(region.channels.applies_to(1));
        let _ = region.end;
    }

    #[test]
    fn commit_updates_existing_region_only() {
        let mut doc = test_document(1000);
        let id = doc.add_region(10, 20, ChannelScope::all());
        doc.begin_region_drag(10, ChannelScope::all());
        if let Selection::Region {
            region_id: ref mut rid,
            ..
        } = doc.selection
        {
            *rid = Some(id);
        }
        doc.update_region_drag(50);
        doc.finish_region_drag();
        let buffer = doc.buffer.read().unwrap();
        let region = buffer.region(id).unwrap();
        assert_eq!(region.start, 10);
        assert_eq!(region.end, 50);
        assert_eq!(doc.current_position.as_ref().map(|p| p.sample), Some(50));
    }

    #[test]
    fn transient_region_not_persisted() {
        let mut doc = test_document(1000);
        doc.begin_region_drag(10, ChannelScope::all());
        doc.update_region_drag(50);
        doc.finish_region_drag();
        assert!(doc.buffer.read().unwrap().regions.is_empty());
        assert!(matches!(
            doc.selection,
            Selection::Region {
                region_id: None,
                ..
            }
        ));
        assert_eq!(doc.current_position.as_ref().map(|p| p.sample), Some(50));
    }

    #[test]
    fn reverse_drag_normalizes_bounds_and_sets_position_to_end() {
        let mut doc = test_document(1000);
        doc.begin_region_drag(80, ChannelScope::all());
        doc.update_region_drag(10);
        doc.finish_region_drag();
        assert!(matches!(
            doc.selection,
            Selection::Region {
                start: 10,
                end: 80,
                ..
            }
        ));
        assert_eq!(doc.current_position.as_ref().map(|p| p.sample), Some(80));
    }

    #[test]
    fn cut_and_delete_use_selection_span() {
        let mut doc = test_document(100);
        doc.begin_region_drag(10, ChannelScope::all());
        doc.update_region_drag(19);
        doc.finish_region_drag();
        doc.edit_cut();
        assert_eq!(doc.frames(), 90);
        doc.begin_region_drag(0, ChannelScope::all());
        doc.update_region_drag(4);
        doc.finish_region_drag();
        doc.edit_delete();
        assert_eq!(doc.frames(), 90);
    }

    #[test]
    fn edit_ops_without_selection_are_noops() {
        let mut doc = test_document(50);
        doc.edit_cut();
        doc.edit_copy();
        doc.edit_delete();
        doc.edit_remove();
        doc.edit_duplicate();
        doc.edit_trim();
        assert_eq!(doc.frames(), 50);
    }
}
