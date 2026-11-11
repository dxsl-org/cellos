use alloc::collections::BTreeMap;
use api::display::SurfaceRole;
use types::ViError;

use super::{SurfaceState, MAX_SURFACES, MAX_SURFACE_DIM};

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
    ///
    /// Dimensions above [`MAX_SURFACE_DIM`] are refused so later byte-size and
    /// screen-rect arithmetic cannot wrap. `Panel` is a single-owner role: while
    /// another owner has a live panel, a request for it gets an ordinary
    /// decorated `Interactive` window instead of an undecorated top-level surface
    /// that could pose as the desktop shell or a lock screen.
    pub fn create(
        &mut self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        owner: usize,
        role: SurfaceRole,
    ) -> Result<u64, ViError> {
        if w > MAX_SURFACE_DIM || h > MAX_SURFACE_DIM {
            return Err(ViError::InvalidArgument);
        }
        if self.entries.len() >= MAX_SURFACES {
            return Err(ViError::OutOfMemory);
        }
        let role = if role == SurfaceRole::Panel && self.panel_held_by_other(owner) {
            SurfaceRole::Interactive
        } else {
            role
        };
        let cap = self.next_cap;
        self.next_cap += 1;
        self.entries
            .insert(cap, SurfaceState::new(x, y, w, h, owner, role));
        Ok(cap)
    }

    fn panel_held_by_other(&self, owner: usize) -> bool {
        self.entries
            .values()
            .any(|surface| surface.role == SurfaceRole::Panel && surface.owner != owner)
    }

    /// Owner of a surface that currently references `reg_id` as its active,
    /// staged or retired Grant.
    pub fn grant_holder(&self, reg_id: usize) -> Option<usize> {
        self.entries
            .values()
            .find(|surface| surface.grant_ids().contains(&Some(reg_id)))
            .map(|surface| surface.owner)
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
            .any(|surface| surface.is_shown() && surface.damage.is_some())
    }

    /// Find all surfaces owned by `tid` and return their caps.
    /// Owner tid of every surface (with repeats).
    pub fn owners(&self) -> alloc::vec::Vec<usize> {
        self.entries.values().map(|surface| surface.owner).collect()
    }

    pub fn caps_owned_by(&self, tid: usize) -> alloc::vec::Vec<u64> {
        self.entries
            .iter()
            .filter(|(_, surface)| surface.owner == tid)
            .map(|(&cap, _)| cap)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use api::display::PixelFormat;

    use super::*;

    #[test]
    fn oversized_surfaces_are_refused() {
        let mut table = SurfaceTable::new();
        let too_big = crate::surface_table::MAX_SURFACE_DIM + 1;
        assert_eq!(
            table.create(0, 0, too_big, 1, 7, SurfaceRole::Interactive),
            Err(ViError::InvalidArgument)
        );
        assert_eq!(
            table.create(0, 0, 1, too_big, 7, SurfaceRole::Interactive),
            Err(ViError::InvalidArgument)
        );
    }

    #[test]
    fn panel_role_has_one_owner_at_a_time() {
        let mut table = SurfaceTable::new();
        let shell = table.create(0, 0, 8, 8, 7, SurfaceRole::Panel).unwrap();
        let second = table.create(0, 0, 8, 8, 7, SurfaceRole::Panel).unwrap();
        let rival = table.create(0, 0, 8, 8, 9, SurfaceRole::Panel).unwrap();
        assert_eq!(table.get(shell).unwrap().role, SurfaceRole::Panel);
        assert_eq!(table.get(second).unwrap().role, SurfaceRole::Panel);
        assert_eq!(table.get(rival).unwrap().role, SurfaceRole::Interactive);

        table.remove(shell);
        table.remove(second);
        let next = table.create(0, 0, 8, 8, 9, SurfaceRole::Panel).unwrap();
        assert_eq!(table.get(next).unwrap().role, SurfaceRole::Panel);
    }

    #[test]
    fn grant_holder_names_the_surface_owner() {
        let mut table = SurfaceTable::new();
        let cap = table
            .create(0, 0, 8, 8, 7, SurfaceRole::Interactive)
            .unwrap();
        assert_eq!(table.grant_holder(0x4000), None);
        assert!(table
            .get_mut(cap)
            .unwrap()
            .attach_grant(0x4000, 8, 8, PixelFormat::Bgra8888));
        assert_eq!(table.grant_holder(0x4000), Some(7));
    }
}
