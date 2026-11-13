//! Owner-death reclaim for the compositor: which requests create owned state,
//! and what the compositor removes when the owning cell dies.
//!
//! The death watch itself lives in `ostd::service::owners`: `serve` watches a
//! caller's cell while serving its first `CREATE_SURFACE` or `CREATE_VIEW` (no
//! other request creates owned state), and reports its death, or its retirement
//! by a newer generation of the same CellId, through `Handler::on_owner_death`.
//!
//! Surfaces and views are keyed by the creating **thread** tid, while a death
//! names the cell incarnation. The compositor therefore records the thread tids
//! that created state on each owner's behalf; the death reclaims every one.
//!
//! Syscall-free so the host test target can exercise it (ostd syscalls are real
//! Linux syscalls on the host).

use alloc::string::String;
use alloc::vec::Vec;

use api::display::{compositor_ops, Rect};

use crate::input_handler::InputState;
use crate::surface_table::{FrameRegistry, SurfaceTable};
use crate::z_order::ZOrder;

/// Cells watched at once. Larger than any realistic number of GUI clients; a
/// cell that does not fit is served without a watch (fail-open).
pub(crate) const MAX_WATCHED_CELLS: usize = 64;
/// Thread tids recorded per cell: the surface (32) and view (64) table
/// capacities, so a cell cannot own more distinct creators than that.
pub(crate) const MAX_TIDS_PER_CELL: usize = 96;

/// Window state to reclaim for one dead (or superseded) cell generation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Released {
    pub cell_id: u64,
    pub generation: u64,
    pub tids: Vec<usize>,
}

/// Only these requests create state that outlives the call, so only they need
/// the owner to be watched.
pub(crate) fn creates_owned_state(opcode: u8) -> bool {
    matches!(
        opcode,
        compositor_ops::CREATE_SURFACE | compositor_ops::CREATE_VIEW
    )
}

/// Remove every surface, view, z-order entry, focus and grant frame that any of
/// `released`'s thread tids created, and queue the freed rectangles for repaint.
/// Returns the number of surfaces removed.
pub(crate) fn reclaim(
    released: &Released,
    input: &mut InputState,
    table: &mut SurfaceTable,
    frames: &mut FrameRegistry,
    z_order: &mut ZOrder,
    pending_dirty: &mut Option<Rect>,
) -> usize {
    released
        .tids
        .iter()
        .map(|&tid| {
            crate::message_handler::cleanup_owner(tid, input, table, frames, z_order, pending_dirty)
        })
        .sum()
}

/// The one log line for a reclaimed cell, only when something was removed.
pub(crate) fn death_line(released: &Released, removed: usize) -> Option<String> {
    (removed > 0).then(|| {
        alloc::format!(
            "[compositor] owner death: cell {} gen {}: removed {} surfaces",
            released.cell_id,
            released.generation,
            removed
        )
    })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use api::display::SurfaceRole;
    use api::display::{CreateViewRequest, ViewOpResult};
    use api::services::proto::compositor::OWNER_EXITED_RELAY;

    #[test]
    fn only_create_requests_need_a_watch() {
        assert!(creates_owned_state(compositor_ops::CREATE_SURFACE));
        assert!(creates_owned_state(compositor_ops::CREATE_VIEW));
        #[allow(deprecated)]
        let others = [
            0u8,
            0x10,
            OWNER_EXITED_RELAY,
            compositor_ops::WRITE_PIXELS,
            compositor_ops::DAMAGE_SURFACE,
            compositor_ops::MOVE_SURFACE,
            compositor_ops::RAISE_SURFACE,
            compositor_ops::DESTROY_SURFACE,
            compositor_ops::SET_TITLE,
            compositor_ops::ATTACH_GRANT,
            compositor_ops::DESTROY_VIEW,
            compositor_ops::SELECT_VIEW,
            compositor_ops::GET_SCREEN_SIZE,
        ];
        for opcode in others {
            assert!(!creates_owned_state(opcode), "opcode {opcode:#x}");
        }
    }

    #[test]
    fn death_line_only_when_something_was_removed() {
        let released = Released {
            cell_id: 12,
            generation: 3,
            tids: alloc::vec![1],
        };
        assert_eq!(death_line(&released, 0), None);
        assert_eq!(
            death_line(&released, 2).as_deref(),
            Some("[compositor] owner death: cell 12 gen 3: removed 2 surfaces")
        );
    }

    fn view_request() -> CreateViewRequest {
        CreateViewRequest {
            opcode: compositor_ops::CREATE_VIEW,
            role: SurfaceRole::Interactive,
            _pad: [0; 2],
            frame_id: 0,
            frame_generation: 0,
            w: 320,
            h: 200,
            min_w: 100,
            min_h: 100,
            recipient_incarnation: 1,
            file_grant_id: 0,
            request_generation: 1,
            _reserved: 0,
            launch_token: [0; 16],
        }
    }

    struct World {
        input: InputState,
        table: SurfaceTable,
        frames: FrameRegistry,
        z_order: ZOrder,
        dirty: Option<Rect>,
    }

    impl World {
        fn new() -> Self {
            Self {
                input: InputState::new(),
                table: SurfaceTable::new(),
                frames: FrameRegistry::new(),
                z_order: ZOrder::new(),
                dirty: None,
            }
        }

        /// A surface (owned pixels, never a registered grant: no syscall at
        /// cleanup) created by thread `tid`, with one view on it.
        fn window(&mut self, tid: usize, x: i32) -> (u64, u32) {
            let cap = self
                .table
                .create(x, 10, 64, 48, tid, SurfaceRole::Interactive)
                .expect("surface");
            self.z_order.push(cap);
            let view = self
                .frames
                .create_view(&view_request(), tid, cap)
                .expect("view");
            assert_eq!(view.result, ViewOpResult::Success);
            (cap, view.view_id)
        }

        fn reclaim(&mut self, released: &Released) -> usize {
            reclaim(
                released,
                &mut self.input,
                &mut self.table,
                &mut self.frames,
                &mut self.z_order,
                &mut self.dirty,
            )
        }
    }

    fn z_caps(world: &World) -> Vec<u64> {
        world.z_order.iter_bottom_to_top().collect()
    }

    #[test]
    fn owner_death_reclaims_exactly_the_dead_cells_windows() {
        let mut world = World::new();
        // Cell A (root 1000, worker 1001) and cell B (root 2000).
        let (a_root, a_root_view) = world.window(1000, 0);
        let (a_worker, a_worker_view) = world.window(1001, 200);
        let (b_cap, b_view) = world.window(2000, 400);
        assert!(world.input.request_focus(1000, a_root));
        assert_eq!(world.input.selected_cap(), Some(a_root));
        let a_bounds: Vec<Rect> = [a_root, a_worker]
            .iter()
            .map(|&cap| {
                crate::window_decoration::bounds(world.table.get(cap).unwrap().screen_rect())
            })
            .collect();

        // Cell A's death, as recorded by both of its creating threads.
        let dead = Released {
            cell_id: 1,
            generation: 1,
            tids: alloc::vec![1000, 1001],
        };
        assert_eq!(world.reclaim(&dead), 2);

        for cap in [a_root, a_worker] {
            assert!(world.table.get(cap).is_none());
        }
        assert!(world.table.get(b_cap).is_some());
        assert_eq!(z_caps(&world), [b_cap]);
        assert_eq!(world.frames.surface_for_view(a_root_view), None);
        assert_eq!(world.frames.surface_for_view(a_worker_view), None);
        assert_eq!(world.frames.surface_for_view(b_view), Some(b_cap));
        assert_eq!(
            world.input.selected_cap(),
            None,
            "focus on A's surface is gone"
        );

        // The freed rectangles are queued for repaint.
        let dirty = world.dirty.expect("dirty rect");
        for freed in a_bounds {
            assert!(
                dirty.x <= freed.x && dirty.y <= freed.y,
                "{dirty:?} vs {freed:?}"
            );
            assert!(
                dirty.x + dirty.w as i32 >= freed.x + freed.w as i32
                    && dirty.y + dirty.h as i32 >= freed.y + freed.h as i32,
                "{dirty:?} vs {freed:?}"
            );
        }

        // Repeat death: nothing left to remove, B untouched.
        assert_eq!(world.reclaim(&dead), 0);
        assert!(world.table.get(b_cap).is_some());
        assert_eq!(z_caps(&world), [b_cap]);
    }

    #[test]
    fn focus_on_another_cells_surface_survives_a_death() {
        let mut world = World::new();
        let (_a, _) = world.window(1000, 0);
        let (b_cap, _) = world.window(2000, 400);
        assert!(world.input.request_focus(2000, b_cap));
        let dead = Released {
            cell_id: 1,
            generation: 1,
            tids: alloc::vec![1000],
        };
        assert_eq!(world.reclaim(&dead), 1);
        assert_eq!(world.input.selected_cap(), Some(b_cap));
    }

    /// The supervisor `OWNER_EXITED_RELAY` relay still reaches the same routine per tid.
    #[test]
    fn relay_path_cleanup_owner_is_unchanged() {
        let mut world = World::new();
        let (cap, view) = world.window(3000, 0);
        let (other, _) = world.window(3001, 100);
        let removed = crate::message_handler::cleanup_owner(
            3000,
            &mut world.input,
            &mut world.table,
            &mut world.frames,
            &mut world.z_order,
            &mut world.dirty,
        );
        assert_eq!(removed, 1);
        assert!(world.table.get(cap).is_none());
        assert_eq!(world.frames.surface_for_view(view), None);
        assert!(world.table.get(other).is_some());
    }
}
