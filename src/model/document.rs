// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::sync::{Arc, RwLock};

use super::buffer::{Buffer, ChannelScope, Region, RegionId};
use super::composition::{
    map_inclusive_through_inverse, map_inclusive_through_op, map_point_through_inverse,
    map_point_through_op, marker_type_color, Composition, EditId, EditOp, MarkerId,
};
use super::selection::{SamplePosition, Selection};
use super::snap::nearest_zero_crossing;
use crate::components::waveform::WaveformDataProvider;
use crate::progress::ProgressHandle;

const DRAG_THRESHOLD_SAMPLES: usize = 0;

pub struct BufferDocument {
    pub composition: Arc<RwLock<Composition>>,
    pub buffer: Arc<RwLock<Buffer>>,
    pub selection: Selection,
    pub current_position: Option<SamplePosition>,
    pub snap_zero_crossings: bool,
    pub progress: ProgressHandle,
    region_drag_anchor: Option<usize>,
    next_region_id: u64,
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
            progress: ProgressHandle::new(),
            region_drag_anchor: None,
            next_region_id: 1,
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
        self.add_labeled_region(start, end, channels, None)
    }

    pub fn add_labeled_region(
        &mut self,
        start: usize,
        end: usize,
        channels: ChannelScope,
        label: Option<String>,
    ) -> RegionId {
        let id = RegionId(self.next_region_id);
        self.next_region_id += 1;
        let mut region = Region::new(id, start, end, channels);
        if let Some(label) = label {
            region = region.with_label(label);
        }
        self.buffer.write().unwrap().regions.push(region);
        id
    }

    pub fn remove_region(&mut self, id: RegionId) -> bool {
        self.buffer.write().unwrap().remove_region(id)
    }

    pub fn select_range(&mut self, start: usize, stop: usize, channels: ChannelScope) {
        let start = self.clamp_sample(start);
        let stop = self.clamp_sample(stop);
        let (start, end) = Self::normalized_region_bounds(start, stop);
        self.region_drag_anchor = None;
        self.set_current_position_sample(end, channels.clone());
        self.selection = Selection::Region {
            region_id: None,
            start,
            end,
            channels,
        };
    }

    pub fn select_all(&mut self) {
        let frames = self.frames();
        if frames == 0 {
            self.clear_selection();
            return;
        }
        self.select_range(0, frames.saturating_sub(1), ChannelScope::all());
    }

    pub fn clear_selection(&mut self) {
        self.region_drag_anchor = None;
        self.selection = Selection::None;
    }

    pub fn invert_selection(&mut self) {
        let frames = self.frames();
        if frames == 0 {
            self.clear_selection();
            return;
        }
        let last = frames.saturating_sub(1);
        match &self.selection {
            Selection::None | Selection::Position(_) => self.select_all(),
            Selection::Region { start, end, .. } => {
                let start = *start;
                let end = *end;
                if start == 0 && end >= last {
                    self.clear_selection();
                    return;
                }
                let touches_start = start == 0;
                let touches_end = end >= last;
                if touches_start && !touches_end {
                    self.select_range(end.saturating_add(1), last, ChannelScope::all());
                } else if touches_end && !touches_start {
                    self.select_range(0, start.saturating_sub(1), ChannelScope::all());
                } else {
                    let prefix_len = start;
                    let suffix_len = last.saturating_sub(end);
                    if prefix_len >= suffix_len && start > 0 {
                        self.select_range(0, start - 1, ChannelScope::all());
                    } else if end < last {
                        self.select_range(end + 1, last, ChannelScope::all());
                    } else {
                        self.clear_selection();
                    }
                }
            }
        }
    }

    pub fn add_marker(
        &mut self,
        sample: usize,
        marker_type: &str,
        color: [f32; 4],
        note: Option<String>,
    ) -> Option<MarkerId> {
        let sample = self.clamp_sample(sample);
        self.composition
            .write()
            .unwrap()
            .add_marker(sample as u64, marker_type, color, note)
    }

    pub fn add_marker_of_type(&mut self, sample: usize, marker_type: &str) -> Option<MarkerId> {
        let color = marker_type_color(marker_type)
            .or_else(|| marker_type_color(super::composition::default_marker_type()))?;
        self.add_marker(sample, marker_type, color, None)
    }

    pub fn remove_marker(&mut self, id: MarkerId) -> bool {
        self.composition.write().unwrap().remove_marker(id)
    }

    pub fn remove_marker_at(&mut self, sample: usize) -> bool {
        self.composition
            .write()
            .unwrap()
            .remove_marker_at(sample as u64)
    }

    pub fn remove_marker_at_type(&mut self, sample: usize, marker_type: &str) -> bool {
        self.composition
            .write()
            .unwrap()
            .remove_marker_at_type(sample as u64, marker_type)
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

    fn after_tree_changed(&mut self, from_cursor: usize) {
        let to_cursor = self.composition.read().unwrap().edit_cursor();
        self.remap_between_cursors(from_cursor, to_cursor);
        self.clamp_playhead_and_selection();
    }

    fn remap_between_cursors(&mut self, from: usize, to: usize) {
        let ops: Vec<EditOp> = self
            .composition
            .read()
            .unwrap()
            .edits()
            .iter()
            .map(|edit| edit.op.clone())
            .collect();
        if to > from {
            for op in &ops[from + 1..=to] {
                self.remap_through_op(op);
            }
        } else if to < from {
            for op in ops[to + 1..=from].iter().rev() {
                self.remap_through_inverse(op);
            }
        }
    }

    fn remap_through_op(&mut self, op: &EditOp) {
        if let Some(pos) = self.current_position.as_mut() {
            pos.sample = map_point_through_op(pos.sample as u64, op) as usize;
        }
        self.selection = match self.selection.clone() {
            Selection::Region {
                start,
                end,
                channels,
                ..
            } => match map_inclusive_through_op(start as u64, end as u64, op) {
                Some((start, end)) => Selection::Region {
                    region_id: None,
                    start: start as usize,
                    end: end as usize,
                    channels,
                },
                None => Selection::Position(SamplePosition {
                    sample: map_point_through_op(start as u64, op) as usize,
                    channels,
                }),
            },
            Selection::Position(pos) => Selection::Position(SamplePosition {
                sample: map_point_through_op(pos.sample as u64, op) as usize,
                channels: pos.channels,
            }),
            Selection::None => Selection::None,
        };
    }

    fn remap_through_inverse(&mut self, op: &EditOp) {
        if let Some(pos) = self.current_position.as_mut() {
            pos.sample = map_point_through_inverse(pos.sample as u64, op) as usize;
        }
        self.selection = match self.selection.clone() {
            Selection::Region {
                start,
                end,
                channels,
                ..
            } => match map_inclusive_through_inverse(start as u64, end as u64, op) {
                Some((start, end)) => Selection::Region {
                    region_id: None,
                    start: start as usize,
                    end: end as usize,
                    channels,
                },
                None => Selection::Position(SamplePosition {
                    sample: map_point_through_inverse(start as u64, op) as usize,
                    channels,
                }),
            },
            Selection::Position(pos) => Selection::Position(SamplePosition {
                sample: map_point_through_inverse(pos.sample as u64, op) as usize,
                channels: pos.channels,
            }),
            Selection::None => Selection::None,
        };
    }

    fn clamp_playhead_and_selection(&mut self) {
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
                let start = self.clamp_sample(start);
                let end = self.clamp_sample(end);
                let (start, end) = Self::normalized_region_bounds(start, end);
                self.selection = Selection::Region {
                    region_id,
                    start,
                    end,
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
        let from = self.composition.read().unwrap().edit_cursor();
        let ok = self.composition.write().unwrap().undo();
        if ok {
            self.after_tree_changed(from);
        }
        ok
    }

    pub fn edit_redo(&mut self) -> bool {
        let from = self.composition.read().unwrap().edit_cursor();
        let ok = self.composition.write().unwrap().redo();
        if ok {
            self.after_tree_changed(from);
        }
        ok
    }

    pub fn jump_to_edit(&mut self, id: EditId) -> bool {
        let from = self.composition.read().unwrap().edit_cursor();
        let ok = self.composition.write().unwrap().jump_to_edit(id);
        if ok {
            self.after_tree_changed(from);
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
        let from = self.composition.read().unwrap().edit_cursor();
        self.composition.write().unwrap().cut(start, len);
        self.after_tree_changed(from);
    }

    pub fn edit_paste(&mut self) {
        let (at, replace) = if let Some((start, len)) = self.selection_span() {
            (start, len)
        } else {
            (self.caret_frame(), 0)
        };
        let from = self.composition.read().unwrap().edit_cursor();
        let _ = self
            .composition
            .write()
            .unwrap()
            .paste_replacing(at, replace);
        self.after_tree_changed(from);
    }

    pub fn edit_delete(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        let from = self.composition.read().unwrap().edit_cursor();
        self.composition.write().unwrap().delete(start, len);
        self.after_tree_changed(from);
    }

    pub fn edit_remove(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        let from = self.composition.read().unwrap().edit_cursor();
        self.composition.write().unwrap().remove(start, len);
        self.after_tree_changed(from);
    }

    pub fn edit_duplicate(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        let from = self.composition.read().unwrap().edit_cursor();
        self.composition.write().unwrap().duplicate(start, len);
        self.after_tree_changed(from);
    }

    pub fn edit_trim(&mut self) {
        let Some((start, len)) = self.selection_span() else {
            return;
        };
        let from = self.composition.read().unwrap().edit_cursor();
        self.composition.write().unwrap().trim(start, len);
        self.after_tree_changed(from);
    }

    pub fn edit_roll(&mut self, delta: i64) {
        let at = self
            .selection_span()
            .map(|(start, _)| start)
            .unwrap_or_else(|| self.caret_frame());
        let from = self.composition.read().unwrap().edit_cursor();
        self.composition.write().unwrap().roll(at, delta);
        self.after_tree_changed(from);
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

    fn peaks_ready(&self) -> bool {
        self.composition.read().unwrap().can_paint_overview()
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

    #[test]
    fn paste_before_playhead_shifts_position() {
        let mut doc = test_document(100);
        doc.set_position(50, ChannelScope::all());
        doc.composition.write().unwrap().copy(0, 10);
        let from = doc.composition.read().unwrap().edit_cursor();
        doc.composition.write().unwrap().paste(0).unwrap();
        doc.after_tree_changed(from);
        assert_eq!(doc.frames(), 110);
        assert_eq!(doc.current_position.as_ref().map(|p| p.sample), Some(60));
        assert!(doc.edit_undo());
        assert_eq!(doc.frames(), 100);
        assert_eq!(doc.current_position.as_ref().map(|p| p.sample), Some(50));
    }

    #[test]
    fn paste_at_caret_follows_the_shifted_sample() {
        let mut doc = test_document(100);
        doc.begin_region_drag(0, ChannelScope::all());
        doc.update_region_drag(9);
        doc.finish_region_drag();
        doc.edit_copy();
        doc.set_position(50, ChannelScope::all());
        doc.edit_paste();
        assert_eq!(doc.frames(), 110);
        assert_eq!(doc.current_position.as_ref().map(|p| p.sample), Some(60));
    }

    #[test]
    fn invert_selection_covers_edges_and_interior() {
        let mut doc = test_document(100);
        doc.invert_selection();
        assert!(matches!(
            doc.selection,
            Selection::Region {
                start: 0,
                end: 99,
                ..
            }
        ));
        doc.invert_selection();
        assert!(matches!(doc.selection, Selection::None));

        doc.select_range(0, 40, ChannelScope::all());
        doc.invert_selection();
        assert!(matches!(
            doc.selection,
            Selection::Region {
                start: 41,
                end: 99,
                ..
            }
        ));

        doc.select_range(50, 99, ChannelScope::all());
        doc.invert_selection();
        assert!(matches!(
            doc.selection,
            Selection::Region {
                start: 0,
                end: 49,
                ..
            }
        ));

        doc.select_range(10, 20, ChannelScope::all());
        doc.invert_selection();
        assert!(matches!(
            doc.selection,
            Selection::Region {
                start: 21,
                end: 99,
                ..
            }
        ));

        doc.select_range(80, 90, ChannelScope::all());
        doc.invert_selection();
        assert!(matches!(
            doc.selection,
            Selection::Region {
                start: 0,
                end: 79,
                ..
            }
        ));
    }

    #[test]
    fn add_and_remove_marker_at_caret() {
        let mut doc = test_document(100);
        let id = doc.add_marker_of_type(40, "Blue").unwrap();
        assert_eq!(
            doc.composition
                .read()
                .unwrap()
                .markers()
                .get(id)
                .unwrap()
                .frame,
            40
        );
        assert!(doc.remove_marker_at(40));
        assert!(doc.composition.read().unwrap().markers().is_empty());
    }

    #[test]
    fn add_marker_is_unique_per_type_at_position() {
        let mut doc = test_document(100);
        assert!(doc.add_marker_of_type(40, "Blue").is_some());
        assert!(doc.add_marker_of_type(40, "Blue").is_none());
        assert!(doc.add_marker_of_type(40, "Yellow").is_some());
        assert_eq!(doc.composition.read().unwrap().markers().len(), 2);
        assert!(doc.remove_marker_at_type(40, "Blue"));
        assert_eq!(doc.composition.read().unwrap().markers().len(), 1);
    }
}
