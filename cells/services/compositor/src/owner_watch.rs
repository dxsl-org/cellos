//! Owner-death reclaim for the compositor: which cells own windows, and what
//! the receive loop does when the kernel reports one of them dead.
//!
//! Everything here is syscall-free so the host test target can exercise it
//! (ostd syscalls are real Linux syscalls on the host). `main.rs` supplies the
//! two kernel calls through [`WatchSys`].
//!
//! Surfaces and views are keyed by the creating **thread** tid, while the kernel
//! reports the death of the cell **root** tid. A watched cell therefore carries
//! the set of thread tids that created state on its behalf; the death reclaims
//! every one of them. A cell is watched from its first `CREATE_SURFACE` or
//! `CREATE_VIEW`, since no other request creates owned state.

use alloc::string::String;
use alloc::vec::Vec;

use api::caller_identity::CallerIdentity;
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

/// The two kernel calls the watch needs.
pub(crate) trait WatchSys {
    /// Subscribe to the root death of `(cell_id, generation)`. The kernel only
    /// grants this to the registered compositor while it is serving that
    /// principal, i.e. during the receive turn that delivered its message.
    /// Returns the root tid the death will be reported under and the token.
    fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)>;
    /// Cancel a watch token (harmless when already consumed).
    fn cancel(&mut self, token: u64);
}

/// Window state to reclaim for one dead (or superseded) cell generation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Released {
    pub cell_id: u64,
    pub generation: u64,
    pub tids: Vec<usize>,
}

struct Watched {
    cell_id: u64,
    generation: u64,
    root_tid: usize,
    token: u64,
    tids: Vec<usize>,
}

/// What [`OwnerWatches::admit`] asks the caller to do besides serving the request.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Admission {
    /// Older generations of the same CellId whose cell is gone: reclaim them.
    pub stale: Vec<Released>,
    /// A watch could not be established; true only the first time (log once).
    pub unavailable: bool,
}

/// How the receive loop treats one received message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Incoming {
    /// A request from a kernel-attested caller: serve it.
    Serve(CallerIdentity),
    /// The kernel's owner-death record for a watched cell: reclaim it.
    OwnerDeath(Released),
    /// No attributable sender and not a death of a watched cell: ignore.
    Drop,
}

/// Cells whose death the compositor has subscribed to.
pub(crate) struct OwnerWatches {
    cells: Vec<Watched>,
    unavailable_logged: bool,
}

/// Only these requests create state that outlives the call, so only they need
/// the owner to be watched.
pub(crate) fn creates_owned_state(opcode: u8) -> bool {
    matches!(
        opcode,
        compositor_ops::CREATE_SURFACE | compositor_ops::CREATE_VIEW
    )
}

impl OwnerWatches {
    pub(crate) const fn new() -> Self {
        Self {
            cells: Vec::new(),
            unavailable_logged: false,
        }
    }

    fn note_unavailable(&mut self) -> bool {
        !core::mem::replace(&mut self.unavailable_logged, true)
    }

    /// Record that `sender` (a thread of `identity`'s cell) is about to create
    /// owned state, subscribing to the cell's death on first sight. Must run
    /// before the request is dispatched: the kernel only grants the watch while
    /// the cell's message is the one being served.
    ///
    /// A refused watch never denies the request: without it a dead client's
    /// windows are simply not reclaimed, which is the behaviour before the watch
    /// existed. The next create from the cell retries.
    pub(crate) fn admit(
        &mut self,
        identity: &CallerIdentity,
        sender: usize,
        sys: &mut impl WatchSys,
    ) -> Admission {
        let mut admission = Admission::default();
        // Cell 0 / generation 0 is the kernel's own identity, never a client.
        if identity.cell_id == 0 || identity.generation == 0 {
            return admission;
        }
        if let Some(entry) = self
            .cells
            .iter_mut()
            .find(|w| w.cell_id == identity.cell_id && w.generation == identity.generation)
        {
            if !entry.tids.contains(&sender) {
                if entry.tids.len() < MAX_TIDS_PER_CELL {
                    entry.tids.push(sender);
                } else {
                    admission.unavailable = self.note_unavailable();
                }
            }
            return admission;
        }
        if self.cells.len() >= MAX_WATCHED_CELLS {
            admission.unavailable = self.note_unavailable();
            return admission;
        }
        let Some((root_tid, token)) = sys.watch(identity.cell_id, identity.generation) else {
            admission.unavailable = self.note_unavailable();
            return admission;
        };
        // CellIds are reused: an older generation of this CellId is a dead cell
        // whose death record never reached us (or was refused earlier).
        while let Some(index) = self
            .cells
            .iter()
            .position(|w| w.cell_id == identity.cell_id)
        {
            let old = self.cells.swap_remove(index);
            sys.cancel(old.token);
            admission.stale.push(Released {
                cell_id: old.cell_id,
                generation: old.generation,
                tids: old.tids,
            });
        }
        self.cells.push(Watched {
            cell_id: identity.cell_id,
            generation: identity.generation,
            root_tid,
            token,
            tids: alloc::vec![sender],
        });
        admission
    }

    /// The watched cell whose **root** tid is `root_tid`, removed (the kernel
    /// consumed its token with the death). A worker tid, 0 or an unwatched root
    /// never matches.
    pub(crate) fn take_death(&mut self, root_tid: usize) -> Option<Released> {
        if root_tid == 0 {
            return None;
        }
        let index = self.cells.iter().position(|w| w.root_tid == root_tid)?;
        let dead = self.cells.swap_remove(index);
        Some(Released {
            cell_id: dead.cell_id,
            generation: dead.generation,
            tids: dead.tids,
        })
    }
}

/// Classify one message received with `sys_recv_attested` from `sender`.
///
/// `buf` is the whole receive buffer: the kernel writes the 32-byte identity
/// trailer at its end, and writes none for an owner-death record (the caller
/// zeroes the buffer before every receive, so an unwritten trailer reads as
/// absent). A trailer that disagrees with the reported sender is not trusted.
pub(crate) fn classify(buf: &[u8], sender: usize, watches: &mut OwnerWatches) -> Incoming {
    let identity =
        CallerIdentity::from_recv_buf(buf).filter(|identity| identity.sender_tid == sender as u64);
    match identity {
        Some(identity) => Incoming::Serve(identity),
        None => match watches.take_death(sender) {
            Some(dead) => Incoming::OwnerDeath(dead),
            None => Incoming::Drop,
        },
    }
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

    /// Scripted kernel: root tid of a cell is `cell * 1000`.
    #[derive(Default)]
    struct ScriptedSys {
        refuse: bool,
        watches: Vec<(u64, u64)>,
        cancels: Vec<u64>,
        next_token: u64,
    }

    impl WatchSys for ScriptedSys {
        fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)> {
            self.watches.push((cell_id, generation));
            if self.refuse {
                return None;
            }
            self.next_token += 1;
            Some((cell_id as usize * 1000, self.next_token))
        }

        fn cancel(&mut self, token: u64) {
            self.cancels.push(token);
        }
    }

    fn identity(cell: u64, generation: u64, sender: usize) -> CallerIdentity {
        CallerIdentity {
            flags: 0,
            cell_id: cell,
            generation,
            sender_tid: sender as u64,
        }
    }

    /// A receive buffer as the kernel leaves it: payload first, trailer last.
    fn recv_buf(payload: &[u8], trailer: Option<CallerIdentity>) -> Vec<u8> {
        let mut buf = alloc::vec![0u8; api::ipc::IPC_BUF_SIZE];
        buf[..payload.len()].copy_from_slice(payload);
        if let Some(identity) = trailer {
            let start = buf.len() - api::caller_identity::CALLER_IDENTITY_LEN;
            buf[start..].copy_from_slice(&identity.to_trailer());
        }
        buf
    }

    #[test]
    fn only_create_requests_need_a_watch() {
        assert!(creates_owned_state(compositor_ops::CREATE_SURFACE));
        assert!(creates_owned_state(compositor_ops::CREATE_VIEW));
        #[allow(deprecated)]
        let others = [
            0u8,
            0x10,
            0xE2,
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
    fn admit_watches_a_cell_once_and_records_every_creating_tid() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        let id = identity(7, 2, 7000);

        assert_eq!(watches.admit(&id, 7000, &mut sys), Admission::default());
        assert_eq!(sys.watches, [(7, 2)]);
        // Same cell again, from a worker thread: no second kernel watch.
        let worker = identity(7, 2, 7001);
        assert_eq!(watches.admit(&worker, 7001, &mut sys), Admission::default());
        assert_eq!(watches.admit(&id, 7000, &mut sys), Admission::default());
        assert_eq!(sys.watches.len(), 1);
        assert!(sys.cancels.is_empty());

        let dead = watches.take_death(7000).expect("watched root");
        assert_eq!(dead.tids, [7000, 7001]);
        assert_eq!((dead.cell_id, dead.generation), (7, 2));
    }

    #[test]
    fn kernel_identity_is_never_watched() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        for id in [identity(0, 1, 5), identity(5, 0, 5)] {
            assert_eq!(watches.admit(&id, 5, &mut sys), Admission::default());
        }
        assert!(sys.watches.is_empty());
    }

    #[test]
    fn refused_watch_serves_logs_once_and_retries_at_the_next_create() {
        let mut sys = ScriptedSys {
            refuse: true,
            ..Default::default()
        };
        let mut watches = OwnerWatches::new();
        let id = identity(9, 1, 9000);

        assert!(watches.admit(&id, 9000, &mut sys).unavailable);
        assert!(!watches.admit(&id, 9000, &mut sys).unavailable, "log once");
        assert_eq!(sys.watches.len(), 2, "retried at the next create");
        assert!(watches.take_death(9000).is_none(), "nothing recorded");

        sys.refuse = false;
        assert_eq!(watches.admit(&id, 9000, &mut sys), Admission::default());
        assert_eq!(sys.watches.len(), 3);
        assert!(watches.take_death(9000).is_some());
    }

    #[test]
    fn full_table_serves_without_a_watch() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        for cell in 1..=MAX_WATCHED_CELLS as u64 {
            let id = identity(cell, 1, cell as usize * 1000);
            assert_eq!(
                watches.admit(&id, cell as usize * 1000, &mut sys),
                Admission::default()
            );
        }
        let extra = identity(500, 1, 500_000);
        assert!(watches.admit(&extra, 500_000, &mut sys).unavailable);
        assert!(!watches.admit(&extra, 500_000, &mut sys).unavailable);
        assert_eq!(
            sys.watches.len(),
            MAX_WATCHED_CELLS,
            "no kernel call when full"
        );
        assert!(watches.take_death(500_000).is_none());
        // A watched cell is still served and tracked when the table is full.
        let known = identity(3, 1, 3001);
        assert_eq!(watches.admit(&known, 3001, &mut sys), Admission::default());
        assert_eq!(
            watches.take_death(3000).expect("watched").tids,
            [3000, 3001]
        );
    }

    #[test]
    fn full_tid_set_serves_without_recording() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        let id = identity(4, 1, 4000);
        watches.admit(&id, 4000, &mut sys);
        for extra in 1..MAX_TIDS_PER_CELL {
            assert_eq!(
                watches.admit(&id, 4000 + extra, &mut sys),
                Admission::default()
            );
        }
        assert!(watches.admit(&id, 9999, &mut sys).unavailable);
        let dead = watches.take_death(4000).expect("watched");
        assert_eq!(dead.tids.len(), MAX_TIDS_PER_CELL);
        assert!(!dead.tids.contains(&9999));
    }

    #[test]
    fn take_death_matches_the_root_tid_exactly_and_once() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        watches.admit(&identity(6, 1, 6000), 6000, &mut sys);
        watches.admit(&identity(6, 1, 6001), 6001, &mut sys);
        watches.admit(&identity(8, 1, 8000), 8000, &mut sys);

        assert!(watches.take_death(0).is_none());
        assert!(
            watches.take_death(6001).is_none(),
            "a worker tid is not a root"
        );
        assert!(watches.take_death(1234).is_none());
        let dead = watches.take_death(6000).expect("root");
        assert_eq!(dead.tids, [6000, 6001]);
        assert!(
            watches.take_death(6000).is_none(),
            "second death is a no-op"
        );
        assert!(watches.take_death(8000).is_some(), "other cell intact");
    }

    #[test]
    fn newer_generation_sweeps_the_stale_one() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        watches.admit(&identity(5, 1, 5000), 5000, &mut sys);
        watches.admit(&identity(5, 1, 5002), 5002, &mut sys);
        let old_token = sys.next_token;

        let admission = watches.admit(&identity(5, 2, 5100), 5100, &mut sys);
        assert!(!admission.unavailable);
        assert_eq!(
            admission.stale,
            [Released {
                cell_id: 5,
                generation: 1,
                tids: alloc::vec![5000, 5002],
            }]
        );
        assert_eq!(sys.cancels, [old_token]);
        // Both generations map to root 5000 in the script; the new entry is
        // the only one left and the old generation does not come back.
        let dead = watches.take_death(5000).expect("new generation");
        assert_eq!((dead.generation, dead.tids), (2, alloc::vec![5100]));
        assert!(watches.take_death(5000).is_none());
    }

    #[test]
    fn classification_of_received_messages() {
        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        let id = identity(3, 1, 3000);
        watches.admit(&id, 3000, &mut sys);

        // An attested request is served, whatever else is watched.
        let request = recv_buf(&[compositor_ops::CREATE_SURFACE], Some(id));
        assert_eq!(classify(&request, 3000, &mut watches), Incoming::Serve(id));
        // A trailer naming another sender is not trusted: dropped, not served.
        let forged = recv_buf(&[1], Some(identity(3, 1, 3000)));
        assert_eq!(classify(&forged, 4242, &mut watches), Incoming::Drop);
        // Zero-trailer message from an unwatched sender: dropped, table intact.
        let leftover = recv_buf(&[compositor_ops::CREATE_SURFACE], None);
        assert_eq!(classify(&leftover, 4242, &mut watches), Incoming::Drop);
        assert!(
            watches.take_death(3000).is_some(),
            "table not mutated by a drop"
        );

        // The kernel's death record: no trailer, sender is the watched root.
        watches.admit(&id, 3000, &mut sys);
        let death = recv_buf(&[0, 0, 0, 0, 0, 0, 0, 0], None);
        // A worker tid is not a death.
        assert_eq!(classify(&death, 3001, &mut watches), Incoming::Drop);
        match classify(&death, 3000, &mut watches) {
            Incoming::OwnerDeath(dead) => assert_eq!(dead.tids, [3000]),
            other => panic!("expected owner death, got {other:?}"),
        }
        // Leftover zero-trailer message of the dead client after its death.
        assert_eq!(classify(&leftover, 3000, &mut watches), Incoming::Drop);
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
        world.input.set_focus(1000, Some(a_root));
        assert_eq!(world.input.selected_cap(), Some(a_root));
        let a_bounds: Vec<Rect> = [a_root, a_worker]
            .iter()
            .map(|&cap| {
                crate::window_decoration::bounds(world.table.get(cap).unwrap().screen_rect())
            })
            .collect();

        let mut sys = ScriptedSys::default();
        let mut watches = OwnerWatches::new();
        watches.admit(&identity(1, 1, 1000), 1000, &mut sys);
        watches.admit(&identity(1, 1, 1001), 1001, &mut sys);
        watches.admit(&identity(2, 1, 2000), 2000, &mut sys);

        let death = recv_buf(&[0; 8], None);
        let Incoming::OwnerDeath(dead) = classify(&death, 1000, &mut watches) else {
            panic!("owner death expected");
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
        world.input.set_focus(2000, Some(b_cap));
        let dead = Released {
            cell_id: 1,
            generation: 1,
            tids: alloc::vec![1000],
        };
        assert_eq!(world.reclaim(&dead), 1);
        assert_eq!(world.input.selected_cap(), Some(b_cap));
    }

    /// The supervisor `0xE2` relay still reaches the same routine per tid.
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
