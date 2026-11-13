//! System palette for compositor-drawn chrome, set by the shell.

use ostd::syscall::sys_try_send;
use wire::display::{AppearanceChanged, AppearancePalette, SetAppearance, SurfaceRole};

use crate::surface_table::SurfaceTable;

/// Chrome palette plus the authority and revision that set it.
///
/// Only the owner of a Panel surface (the system shell; the table lets one
/// owner hold Panels at a time) may change it. Revisions are scoped to that
/// owner, so a restarted shell starting again at revision 1 is accepted while
/// a stale or replayed request from the same shell is refused.
pub struct Appearance {
    palette: AppearancePalette,
    owner: usize,
    revision: u32,
    /// Palette changes since boot, from 1; announced to clients.
    generation: u32,
}

impl Appearance {
    pub const fn new() -> Self {
        Self {
            palette: AppearancePalette::Light,
            owner: 0,
            revision: 0,
            generation: 1,
        }
    }

    pub const fn palette(&self) -> AppearancePalette {
        self.palette
    }

    /// The event announcing the current palette.
    pub const fn event(&self) -> AppearanceChanged {
        AppearanceChanged {
            palette: self.palette,
            generation: self.generation,
        }
    }

    /// Tell `owner` the current palette; never blocks (a full queue drops it,
    /// the next change or window attach repeats it).
    pub fn announce_to(&self, owner: usize) {
        let _ = sys_try_send(owner, &self.event().encode());
    }

    /// Tell every distinct surface owner the current palette.
    pub fn broadcast(&self, table: &SurfaceTable) {
        let mut owners = table.owners();
        owners.sort_unstable();
        owners.dedup();
        for owner in owners {
            self.announce_to(owner);
        }
    }

    /// Apply `request` from `sender`; returns whether the visible palette changed.
    pub fn apply(&mut self, request: &SetAppearance, sender: usize, table: &SurfaceTable) -> bool {
        let is_shell_panel = table
            .get(u64::from(request.cap))
            .is_some_and(|surface| surface.owner == sender && surface.role == SurfaceRole::Panel);
        if !is_shell_panel || (sender == self.owner && request.revision <= self.revision) {
            return false;
        }
        let changed = self.palette != request.palette;
        self.palette = request.palette;
        self.owner = sender;
        self.revision = request.revision;
        if changed {
            self.generation = self.generation.wrapping_add(1).max(1);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_panel_owner_sets_it_with_growing_revisions() {
        let mut table = SurfaceTable::new();
        let panel = table.create(0, 0, 8, 8, 7, SurfaceRole::Panel).unwrap();
        let window = table
            .create(0, 0, 8, 8, 9, SurfaceRole::Interactive)
            .unwrap();
        let mut appearance = Appearance::new();
        let request =
            |cap: u64, palette, revision| SetAppearance::new(cap as u32, palette, revision);

        assert!(!appearance.apply(&request(window, AppearancePalette::Dark, 1), 9, &table));
        assert!(
            !appearance.apply(&request(panel, AppearancePalette::Dark, 1), 9, &table),
            "not the owner"
        );
        assert!(appearance.apply(&request(panel, AppearancePalette::Dark, 1), 7, &table));
        assert!(
            !appearance.apply(&request(panel, AppearancePalette::Paper, 1), 7, &table),
            "replay"
        );
        assert_eq!(appearance.palette(), AppearancePalette::Dark);
        assert!(appearance.apply(&request(panel, AppearancePalette::Paper, 2), 7, &table));

        // The shell restarts: its new Panel starts over at revision 1.
        let _ = table.remove(panel);
        let reborn = table.create(0, 0, 8, 8, 11, SurfaceRole::Panel).unwrap();
        assert!(appearance.apply(&request(reborn, AppearancePalette::Light, 1), 11, &table));
        assert_eq!(appearance.palette(), AppearancePalette::Light);
        // Three visible changes after boot's generation 1.
        assert_eq!(appearance.event().generation, 4);
    }
}
