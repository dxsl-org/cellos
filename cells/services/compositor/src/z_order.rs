//! Surface z-order (paint order) management.
//!
//! Surfaces are stored in a `Vec` ordered bottom-to-top.  Index 0 is the
//! bottommost surface; the last element is always on top.

extern crate alloc;
use alloc::vec::Vec;

/// Ordered paint list (bottom index = 0, top = last).
pub struct ZOrder {
    caps: Vec<u64>,
}

impl ZOrder {
    pub fn new() -> Self {
        Self { caps: Vec::new() }
    }

    /// Add a new surface at the top of the stack.
    pub fn push(&mut self, cap: u64) {
        if !self.caps.contains(&cap) {
            self.caps.push(cap);
        }
    }

    /// Remove a surface from the z-order.
    pub fn remove(&mut self, cap: u64) {
        self.caps.retain(|&c| c != cap);
    }

    /// Raise a surface to the top.
    pub fn raise(&mut self, cap: u64) {
        self.remove(cap);
        self.caps.push(cap);
    }

    /// Iterate from bottom to top (paint order).
    pub fn iter_bottom_to_top(&self) -> impl Iterator<Item = u64> + '_ {
        self.caps.iter().copied()
    }

    /// Iterate from top to bottom (hit-test order: frontmost surface wins).
    pub fn iter_top_to_bottom(&self) -> impl Iterator<Item = u64> + '_ {
        self.caps.iter().rev().copied()
    }
}

impl Default for ZOrder {
    fn default() -> Self {
        Self::new()
    }
}
