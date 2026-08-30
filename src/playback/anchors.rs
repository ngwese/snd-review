use crate::model::Buffer;

pub fn collect_anchors(buffer: &Buffer) -> Vec<usize> {
    let mut anchors: Vec<usize> = buffer
        .markers
        .iter()
        .map(|m| m.sample)
        .chain(buffer.regions.iter().flat_map(|r| [r.start, r.end]))
        .collect();
    anchors.sort_unstable();
    anchors.dedup();
    anchors
}

pub fn previous_anchor(anchors: &[usize], pos: usize) -> Option<usize> {
    anchors.iter().copied().filter(|a| *a < pos).max()
}

pub fn next_anchor(anchors: &[usize], pos: usize) -> Option<usize> {
    anchors.iter().copied().filter(|a| *a > pos).min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::DecodedAudio;
    use crate::model::buffer::{ChannelScope, Marker, MarkerId, Region, RegionId};

    fn test_buffer() -> Buffer {
        Buffer {
            audio: DecodedAudio {
                sample_rate: 44100,
                channels: vec![vec![0.0; 1000]],
                peaks: vec![vec![]],
            },
            source: None,
            regions: vec![Region::new(RegionId(1), 100, 200, ChannelScope::all())],
            markers: vec![Marker {
                id: MarkerId(1),
                sample: 50,
                channels: ChannelScope::all(),
                color: None,
                label_type: None,
                message: None,
            }],
        }
    }

    #[test]
    fn collects_markers_and_region_bounds() {
        let anchors = collect_anchors(&test_buffer());
        assert_eq!(anchors, vec![50, 100, 200]);
    }

    #[test]
    fn navigates_anchors() {
        let anchors = collect_anchors(&test_buffer());
        assert_eq!(previous_anchor(&anchors, 150), Some(100));
        assert_eq!(next_anchor(&anchors, 150), Some(200));
        assert_eq!(next_anchor(&anchors, 250), None);
    }
}
