//! Tile groups: frames glued edge to edge. The registry keeps only which
//! frames are glued along which side; where the shared edges are is read off
//! the windows themselves (`crate::tiling`), so it can never drift from them.

use alloc::vec::Vec;

use super::FrameRegistry;

/// Frames one tile group holds at most.
pub const MAX_GROUP: usize = 16;

/// Which side of frame `a` frame `b` is glued to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// `b`'s left edge meets `a`'s right edge.
    Right,
    /// `b`'s top edge meets `a`'s bottom edge.
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Join {
    pub a: u32,
    pub b: u32,
    pub side: Side,
}

impl FrameRegistry {
    /// Glue frame `b` to the `side` of frame `a`, joining (and merging) their
    /// groups. Refused when either frame is gone, they are already glued, or
    /// the merged group would exceed [`MAX_GROUP`].
    pub fn glue(&mut self, a: u32, b: u32, side: Side) -> bool {
        if a == b || !self.frames.contains_key(&a) || !self.frames.contains_key(&b) {
            return false;
        }
        if self
            .joins
            .iter()
            .any(|j| (j.a, j.b) == (a, b) || (j.a, j.b) == (b, a))
        {
            return false;
        }
        let mut merged = self.tile_group(a);
        for frame in self.tile_group(b) {
            if !merged.contains(&frame) {
                merged.push(frame);
            }
        }
        if merged.len() > MAX_GROUP {
            return false;
        }
        self.joins.push(Join { a, b, side });
        true
    }

    /// Take `frame` out of its tile group; the rest may split in two.
    pub fn untile(&mut self, frame: u32) {
        self.joins.retain(|j| j.a != frame && j.b != frame);
        self.group_minimized.retain(|&f| f != frame);
    }

    /// Joins touching `frame`.
    pub fn joins_of(&self, frame: u32) -> Vec<Join> {
        self.joins
            .iter()
            .copied()
            .filter(|j| j.a == frame || j.b == frame)
            .collect()
    }

    /// Remember that the user minimized `frames` together (window menu).
    pub fn mark_group_minimized(&mut self, frames: &[u32]) {
        for &frame in frames {
            if !self.group_minimized.contains(&frame) {
                self.group_minimized.push(frame);
            }
        }
    }

    /// Whether `frame` went down with its group, and forget that it did.
    pub fn take_group_minimized(&mut self, frame: u32) -> bool {
        let before = self.group_minimized.len();
        self.group_minimized.retain(|&f| f != frame);
        self.group_minimized.len() != before
    }

    pub fn is_tiled(&self, frame: u32) -> bool {
        self.joins.iter().any(|j| j.a == frame || j.b == frame)
    }

    /// Every frame glued to `frame` directly or through others, `frame` first.
    pub fn tile_group(&self, frame: u32) -> Vec<u32> {
        let mut group = alloc::vec![frame];
        let mut index = 0;
        while index < group.len() {
            let current = group[index];
            for join in &self.joins {
                let other = if join.a == current {
                    join.b
                } else if join.b == current {
                    join.a
                } else {
                    continue;
                };
                if !group.contains(&other) {
                    group.push(other);
                }
            }
            index += 1;
        }
        group
    }
}
