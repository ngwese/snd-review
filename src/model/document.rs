use super::buffer::{Buffer, ChannelScope, MarkerId, Region, RegionId};
use super::selection::{SamplePosition, Selection};
use super::snap::nearest_zero_crossing_default;
use crate::components::waveform::WaveformDataProvider;

const DRAG_THRESHOLD_SAMPLES: usize = 0;

pub struct BufferDocument {
    pub buffer: Buffer,
    pub selection: Selection,
    pub current_position: Option<SamplePosition>,
    pub snap_zero_crossings: bool,
    region_drag_anchor: Option<usize>,
    next_region_id: u64,
    next_marker_id: u64,
}

impl BufferDocument {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            buffer,
            selection: Selection::None,
            current_position: None,
            snap_zero_crossings: false,
            region_drag_anchor: None,
            next_region_id: 1,
            next_marker_id: 1,
        }
    }

    pub fn toggle_zero_crossing_snap(&mut self) {
        self.snap_zero_crossings = !self.snap_zero_crossings;
    }

    fn snap_channel(&self, channel: usize, sample: usize) -> usize {
        if !self.snap_zero_crossings {
            return sample;
        }
        let samples = WaveformDataProvider::channel_samples(&self.buffer.audio, channel);
        nearest_zero_crossing_default(samples, sample)
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
        let max = self.buffer.frames().saturating_sub(1);
        sample.min(max)
    }

    pub fn sample_to_secs(&self, sample: usize) -> f64 {
        let rate = self.buffer.audio.sample_rate;
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
        self.selection = Selection::Position(SamplePosition { sample, channels: scope });
    }

    pub fn select_region_at(&mut self, sample: usize, lane: usize, scope: ChannelScope) {
        let sample = self.clamp_sample(self.snap_sample(&scope, sample));
        if let Some(id) = self.hit_test_region(sample, lane) {
            if let Some(region) = self.buffer.region(id).cloned() {
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
        self.set_current_position_sample(end, channels);
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
            self.selection = Selection::Position(SamplePosition { sample: start, channels });
            return;
        }

        self.set_current_position_sample(end, channels.clone());

        if let Some(id) = region_id {
            if let Some(region) = self.buffer.region_mut(id) {
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
        self.buffer.regions.push(region);
        id
    }

    pub fn add_marker(
        &mut self,
        sample: usize,
        channels: ChannelScope,
    ) -> MarkerId {
        let id = MarkerId(self.next_marker_id);
        self.next_marker_id += 1;
        self.buffer.markers.push(super::buffer::Marker {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::DecodedAudio;

    fn test_document(frames: usize) -> BufferDocument {
        let audio = DecodedAudio {
            sample_rate: 44100,
            channels: vec![vec![0.0; frames], vec![0.0; frames]],
            peaks: vec![vec![(0.0, 0.0); (frames + 255) / 256]; 2],
        };
        BufferDocument::new(Buffer {
            audio,
            source: None,
            regions: vec![],
            markers: vec![],
        })
    }

    #[test]
    fn full_region_spans_buffer() {
        let doc = test_document(1000);
        let region = doc.buffer.full_region();
        assert_eq!(region.start, 0);
        assert_eq!(region.end, 999);
        assert!(region.channels.applies_to(0));
        assert!(region.channels.applies_to(1));
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
        let region = doc.buffer.region(id).unwrap();
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
        assert!(doc.buffer.regions.is_empty());
        assert!(matches!(doc.selection, Selection::Region { region_id: None, .. }));
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
}
