use alloc::collections::BTreeMap;
use api::display::SurfaceRole;
use types::ViError;

use super::{SurfaceState, MAX_SURFACES};

/// CapId-keyed surface registry.
#[derive(Default)]
pub struct SurfaceTable {
    entries: BTreeMap<u64, SurfaceState>,
    next_cap: u64,
}

impl SurfaceTable {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_cap: 1,
        }
    }

    /// Allocate a new surface slot and return its CapId.
    pub fn create(
        &mut self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        owner: usize,
        role: SurfaceRole,
    ) -> Result<u64, ViError> {
        if self.entries.len() >= MAX_SURFACES {
            return Err(ViError::OutOfMemory);
        }
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries
            .insert(cap, SurfaceState::new(x, y, w, h, owner, role));
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

    /// Returns true if a visible surface has accumulated damage.
    pub fn has_damage(&self) -> bool {
        self.entries
            .values()
            .any(|surface| surface.is_visible_for_paint() && surface.damage.is_some())
    }

    /// Find all surfaces owned by `tid` and return their caps.
    pub fn caps_owned_by(&self, tid: usize) -> alloc::vec::Vec<u64> {
        self.entries
            .iter()
            .filter(|(_, surface)| surface.owner == tid)
            .map(|(&cap, _)| cap)
            .collect()
    }
}
