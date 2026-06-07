//! CapId-keyed surface state table.
//!
//! Each surface has a pixel buffer, screen position, and damage accumulator.
//! The compositor owns all pixel data — client cells write via `WRITE_PIXELS` IPC.

extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap};
use api::display::Rect;
use types::ViError;

/// Maximum number of simultaneous surfaces.
pub const MAX_SURFACES: usize = 16;

/// State for one live surface.
pub struct SurfaceState {
    /// Screen position.
    pub x: i32,
    pub y: i32,
    /// Dimensions in pixels.
    pub w: u32,
    pub h: u32,
    /// BGRA8888 pixel buffer (`w * h * 4` bytes).
    pub pixels: Box<[u8]>,
    /// Accumulated damage since last flush.  `None` = no damage.
    pub damage: Option<Rect>,
    /// TID of the cell that created this surface (input routing target).
    pub owner: usize,
}

impl SurfaceState {
    /// Allocate a new zeroed (transparent black) surface for `owner` (their TID).
    pub fn new(x: i32, y: i32, w: u32, h: u32, owner: usize) -> Self {
        let len = (w * h * 4) as usize;
        let pixels = alloc::vec![0u8; len].into_boxed_slice();
        Self { x, y, w, h, pixels, damage: None, owner }
    }

    /// Write `data` (BGRA8888) into the sub-rect `(px, py, pw, ph)`.
    pub fn write_pixels(&mut self, px: i32, py: i32, pw: u32, ph: u32, data: &[u8]) {
        let expected = (pw * ph * 4) as usize;
        if data.len() < expected { return; }
        let stride = self.w as usize * 4;
        for row in 0..ph as usize {
            let dst_off = (py as usize + row) * stride + px as usize * 4;
            let src_off = row * pw as usize * 4;
            let row_bytes = pw as usize * 4;
            if dst_off + row_bytes <= self.pixels.len() {
                self.pixels[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&data[src_off..src_off + row_bytes]);
            }
        }
        // Accumulate damage.
        let new_dmg = Rect { x: px, y: py, w: pw, h: ph };
        self.damage = Some(match self.damage {
            Some(existing) => existing.union(&new_dmg),
            None => new_dmg,
        });
    }

    /// Clear the damage accumulator after a flush.
    pub fn clear_damage(&mut self) { self.damage = None; }

    /// Bounding rect of this surface on screen.
    pub fn screen_rect(&self) -> Rect {
        Rect { x: self.x, y: self.y, w: self.w, h: self.h }
    }
}

/// CapId-keyed surface registry.
#[derive(Default)]
pub struct SurfaceTable {
    entries: BTreeMap<u64, SurfaceState>,
    next_cap: u64,
}

impl SurfaceTable {
    pub fn new() -> Self { Self { entries: BTreeMap::new(), next_cap: 1 } }

    /// Allocate a new surface and return its CapId.
    ///
    /// `owner` is the TID of the creating cell (used for input focus routing).
    ///
    /// # Errors
    /// Returns `OutOfMemory` if `MAX_SURFACES` is already reached.
    pub fn create(&mut self, x: i32, y: i32, w: u32, h: u32, owner: usize) -> Result<u64, ViError> {
        if self.entries.len() >= MAX_SURFACES { return Err(ViError::OutOfMemory); }
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries.insert(cap, SurfaceState::new(x, y, w, h, owner));
        Ok(cap)
    }

    /// Look up a surface mutably.
    pub fn get_mut(&mut self, cap: u64) -> Option<&mut SurfaceState> {
        self.entries.get_mut(&cap)
    }

    /// Look up a surface immutably.
    pub fn get(&self, cap: u64) -> Option<&SurfaceState> {
        self.entries.get(&cap)
    }

    /// Remove a surface.
    pub fn remove(&mut self, cap: u64) -> Option<SurfaceState> {
        self.entries.remove(&cap)
    }
}
