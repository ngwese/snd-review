// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use super::edl::{Edit, EditOp};
use super::tree::ClipTree;

/// Half-open timeline range `[start, end)` in frames.
pub type FrameRange = (u64, u64);

/// Ranges this op introduced on the tree after it was applied.
pub fn landing_ranges(op: &EditOp, pre: &ClipTree) -> Vec<FrameRange> {
    match op {
        EditOp::Init | EditOp::Copy { .. } => Vec::new(),
        EditOp::Cut { .. } | EditOp::Remove { .. } => Vec::new(),
        EditOp::Delete { start, len } if *len > 0 => vec![(*start, start.saturating_add(*len))],
        EditOp::Delete { .. } => Vec::new(),
        EditOp::Paste { at, len } if *len > 0 => vec![(*at, at.saturating_add(*len))],
        EditOp::Paste { .. } => Vec::new(),
        EditOp::Trim { len, .. } if *len > 0 => vec![(0, *len)],
        EditOp::Trim { .. } => Vec::new(),
        EditOp::Duplicate { start, len } if *len > 0 => {
            let insert_at = start.saturating_add(*len);
            vec![(insert_at, insert_at.saturating_add(*len))]
        }
        EditOp::Duplicate { .. } => Vec::new(),
        EditOp::Move { from, len, dest } if *len > 0 => {
            let insert_at = if *dest > *from {
                dest.saturating_sub(*len)
            } else {
                *dest
            };
            vec![(insert_at, insert_at.saturating_add(*len))]
        }
        EditOp::Move { .. } => Vec::new(),
        EditOp::Roll { at, .. } => pre
            .at(*at)
            .map(|span| (span.start, span.end()))
            .into_iter()
            .filter(|(start, end)| start < end)
            .collect(),
    }
}

fn insert_shift(start: u64, end: u64, at: u64, len: u64) -> Vec<FrameRange> {
    if start >= end {
        return Vec::new();
    }
    if end <= at {
        vec![(start, end)]
    } else if start >= at {
        vec![(start.saturating_add(len), end.saturating_add(len))]
    } else {
        let mut out = Vec::new();
        if start < at {
            out.push((start, at));
        }
        if at < end {
            out.push((at.saturating_add(len), end.saturating_add(len)));
        }
        out
    }
}

fn remove_shift(start: u64, end: u64, at: u64, len: u64) -> Vec<FrameRange> {
    if start >= end || len == 0 {
        return if start < end {
            vec![(start, end)]
        } else {
            Vec::new()
        };
    }
    let removed_end = at.saturating_add(len);
    if end <= at {
        vec![(start, end)]
    } else if start >= removed_end {
        vec![(start.saturating_sub(len), end.saturating_sub(len))]
    } else {
        let mut out = Vec::new();
        if start < at {
            out.push((start, at));
        }
        if end > removed_end {
            out.push((at, end.saturating_sub(len)));
        }
        out.into_iter().filter(|(start, end)| start < end).collect()
    }
}

/// Map a range through an op: input is pre-op coordinates, output is post-op.
pub fn map_range_through_op(start: u64, end: u64, op: &EditOp) -> Vec<FrameRange> {
    if start >= end {
        return Vec::new();
    }
    match op {
        EditOp::Init | EditOp::Copy { .. } | EditOp::Roll { .. } | EditOp::Delete { .. } => {
            vec![(start, end)]
        }
        EditOp::Paste { at, len } => insert_shift(start, end, *at, *len),
        EditOp::Duplicate { start: ds, len } => {
            insert_shift(start, end, ds.saturating_add(*len), *len)
        }
        EditOp::Cut { start: at, len } | EditOp::Remove { start: at, len } => {
            remove_shift(start, end, *at, *len)
        }
        EditOp::Trim { start: keep, len } => {
            let clipped_start = start.max(*keep);
            let clipped_end = end.min(keep.saturating_add(*len));
            if clipped_start < clipped_end {
                vec![(
                    clipped_start.saturating_sub(*keep),
                    clipped_end.saturating_sub(*keep),
                )]
            } else {
                Vec::new()
            }
        }
        EditOp::Move { from, len, dest } => map_through_move(start, end, *from, *len, *dest),
    }
}

fn map_through_move(start: u64, end: u64, from: u64, len: u64, dest: u64) -> Vec<FrameRange> {
    if len == 0 || start >= end {
        return if start < end {
            vec![(start, end)]
        } else {
            Vec::new()
        };
    }
    let moved_end = from.saturating_add(len);
    let insert_at = if dest > from {
        dest.saturating_sub(len)
    } else {
        dest
    };
    let mut out = Vec::new();
    if start < moved_end && end > from {
        let inner_start = start.max(from);
        let inner_end = end.min(moved_end);
        let mapped_start = insert_at.saturating_add(inner_start.saturating_sub(from));
        let mapped_end = insert_at.saturating_add(inner_end.saturating_sub(from));
        if mapped_start < mapped_end {
            out.push((mapped_start, mapped_end));
        }
    }
    for (outside_start, outside_end) in remove_shift(start, end, from, len) {
        out.extend(insert_shift(outside_start, outside_end, insert_at, len));
    }
    out.into_iter().filter(|(start, end)| start < end).collect()
}

fn map_ranges_through_ops(mut ranges: Vec<FrameRange>, ops: &[Edit]) -> Vec<FrameRange> {
    for edit in ops {
        ranges = ranges
            .into_iter()
            .flat_map(|(start, end)| map_range_through_op(start, end, &edit.op))
            .collect();
    }
    ranges
}

/// Landing ranges for `edits[index]`, mapped onto the tree at `cursor`.
pub fn ranges_for_edit(edits: &[Edit], cursor: usize, index: usize) -> Vec<FrameRange> {
    if index == 0 || index > cursor || index >= edits.len() {
        return Vec::new();
    }
    let ranges = landing_ranges(&edits[index].op, &edits[index - 1].snapshot);
    map_ranges_through_ops(ranges, &edits[index + 1..=cursor])
}

/// Landing ranges for every applied user edit, mapped onto the tree at `cursor`.
///
/// Adjacent ranges from different edits are kept separate so the waveform can
/// draw a gap between them.
pub fn modified_ranges(edits: &[Edit], cursor: usize) -> Vec<FrameRange> {
    if edits.is_empty() {
        return Vec::new();
    }
    let cursor = cursor.min(edits.len() - 1);
    let mut all = Vec::new();
    for index in 1..=cursor {
        all.extend(ranges_for_edit(edits, cursor, index));
    }
    all.sort_by_key(|&(start, _)| start);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::composition::clip::{Clip, ClipId};
    use crate::model::composition::edl::{Edit, EditId};
    use crate::model::composition::media::MediaId;
    use crate::model::composition::tree::ClipTree;

    fn media_tree(len: u64) -> ClipTree {
        ClipTree::from_clip(Clip::from_media(ClipId(1), MediaId(0), 0, len))
    }

    fn edit(id: u64, op: EditOp, snapshot: ClipTree) -> Edit {
        Edit {
            id: EditId(id),
            op,
            snapshot,
        }
    }

    #[test]
    fn delete_lands_in_place() {
        let pre = media_tree(20);
        let ranges = landing_ranges(&EditOp::Delete { start: 5, len: 5 }, &pre);
        assert_eq!(ranges, vec![(5, 10)]);
    }

    #[test]
    fn paste_and_duplicate_land_on_inserted_block() {
        let pre = media_tree(10);
        assert_eq!(
            landing_ranges(&EditOp::Paste { at: 2, len: 4 }, &pre),
            vec![(2, 6)]
        );
        assert_eq!(
            landing_ranges(&EditOp::Duplicate { start: 0, len: 3 }, &pre),
            vec![(3, 6)]
        );
    }

    #[test]
    fn later_insert_shifts_earlier_delete() {
        let edits = vec![
            edit(0, EditOp::Init, media_tree(20)),
            edit(1, EditOp::Delete { start: 5, len: 5 }, media_tree(20)),
            edit(2, EditOp::Paste { at: 0, len: 4 }, media_tree(24)),
        ];
        assert_eq!(ranges_for_edit(&edits, 2, 1), vec![(9, 14)]);
        assert_eq!(modified_ranges(&edits, 2), vec![(0, 4), (9, 14)]);
    }

    #[test]
    fn remove_drops_covered_range_and_shifts_the_rest() {
        assert_eq!(
            map_range_through_op(0, 10, &EditOp::Remove { start: 2, len: 3 }),
            vec![(0, 2), (2, 7)]
        );
        assert_eq!(
            map_range_through_op(8, 12, &EditOp::Remove { start: 2, len: 3 }),
            vec![(5, 9)]
        );
    }

    #[test]
    fn move_relocates_the_moved_block() {
        assert_eq!(
            map_range_through_op(
                10,
                20,
                &EditOp::Move {
                    from: 10,
                    len: 10,
                    dest: 50
                }
            ),
            vec![(40, 50)]
        );
    }
}
