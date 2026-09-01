// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

mod clip;
mod composition;
mod edit_ranges;
mod edl;
mod media;
mod pager;
mod tree;

pub use clip::{Clip, ClipCache, ClipId, ClipMarker, ClipMarkerId, ClipSource, ClipSpan};
pub use composition::{Clipboard, Composition, FramesIter};
pub use edit_ranges::{
    map_inclusive_through_inverse, map_inclusive_through_op, map_point_through_inverse,
    map_point_through_op,
};
pub use edl::{Edit, EditId, EditOp, Edl, InitialState, ProjectFile};
pub use media::{MediaId, MediaPool, MediaRef};
pub use pager::{BlockPager, BLOCK_FRAMES};
pub use tree::ClipTree;
