use types::CellId;

use super::snapshot::{exclude_proven_audit_delta, snapshot, snapshot_matches};

const VFS_TID: usize = 50_101;
const INPUT_TID: usize = 50_102;
const SERVICE_TID: usize = 50_103;
const REPLACEMENT_TID: usize = 50_104;
const QUOTA_TID: usize = 50_105;
const ROUTE_TID: usize = 50_106;
const VFS_CELL: CellId = CellId(21);
const INPUT_CELL: CellId = CellId(22);
const SERVICE_CELL: CellId = CellId(23);
const REPLACEMENT_CELL: CellId = CellId(24);
const QUOTA_CELL: CellId = CellId(25);
const ROUTE_CELL: CellId = CellId(26);
const SERVICE_ID: u16 = 0x7f01;
/// A flagged row standing in for VFS, so the corpus also proves registration
/// flags are neither changed by a denial nor left behind by teardown.
const VFS_SERVICE_ID: u16 = 0x7f02;

/// Run a denial against the same populated owners that production admission
/// observes. `audit_bytes` is the documented request evidence the denial
/// appends; fixture installation and teardown must append no audit records.
pub(super) fn with_populated_baseline(
    case: &'static str,
    audit_bytes: usize,
    contract: impl FnOnce() -> bool,
) -> bool {
    let original = snapshot();
    let baseline = SharedStateBaseline::install();
    let populated = baseline.owners_are_populated();
    if !populated {
        log::error!(
            "ATOMIC_PUBLICATION_{}: MISMATCH stage=fixture-install field=populated-owners",
            case,
        );
    }
    let passed = contract();
    let after_contract = snapshot();
    drop(baseline);
    let mut after_teardown = snapshot();
    let audit_preserved =
        exclude_proven_audit_delta(&original, &after_contract, &mut after_teardown, audit_bytes);
    if !audit_preserved {
        log::error!(
            "ATOMIC_PUBLICATION_{}: MISMATCH stage=fixture-teardown field=audit-delta",
            case,
        );
    }
    let restored = snapshot_matches(case, "fixture-teardown", &original, &after_teardown);
    passed && populated && audit_preserved && restored
}

/// Exercise fixture ownership without a denial, so a teardown regression cannot
/// be hidden by a case-specific audit allowance.
pub(super) fn populated_baseline_teardown_restores_state() -> bool {
    let original = snapshot();
    let baseline = SharedStateBaseline::install();
    let populated = baseline.owners_are_populated();
    drop(baseline);
    let restored = snapshot_matches("FIXTURE", "fixture-roundtrip", &original, &snapshot());
    populated && restored
}

struct SharedStateBaseline {
    next_generation: u64,
    block_io_registered: bool,
    input_tid: usize,
}

impl SharedStateBaseline {
    fn install() -> Self {
        let next_generation = crate::task::tcb::cell_generation_snapshot();
        let block_io_registered = crate::loader::block_io_registered_snapshot();
        let input_tid = crate::dev::driver_cell::input_cell_snapshot();
        let [(vfs_cell, vfs_generation), (service_cell, service_generation)] = {
            let mut scheduler = crate::task::SCHEDULER.lock();
            let sched = scheduler.as_mut().expect("atomic corpus needs scheduler");
            for (tid, cell, name) in [
                (VFS_TID, VFS_CELL, "atomic-vfs-owner"),
                (INPUT_TID, INPUT_CELL, "atomic-input-owner"),
                (SERVICE_TID, SERVICE_CELL, "atomic-service-owner"),
                (
                    REPLACEMENT_TID,
                    REPLACEMENT_CELL,
                    "atomic-replacement-owner",
                ),
                (QUOTA_TID, QUOTA_CELL, "atomic-quota-owner"),
                (ROUTE_TID, ROUTE_CELL, "atomic-route-owner"),
            ] {
                let mut task = crate::task::Task::new(tid, cell, name, alloc::vec::Vec::new());
                task.priority = (tid - VFS_TID + 1) as u8;
                task.syscall_allowlist = 1u64 << (tid - VFS_TID);
                let task = alloc::boxed::Box::new(task);
                #[cfg(feature = "p01-trace")]
                crate::memory::heap::p01_owner::task_backing(&task);
                #[cfg(feature = "p01-trace")]
                let _semantic = crate::memory::heap::p01_owner::creation(
                    None,
                    crate::memory::heap::p01_owner::SCHEDULER_SHARED,
                );
                assert!(sched.tasks.insert(tid, task).is_none());
            }
            [VFS_TID, SERVICE_TID].map(|tid| {
                sched
                    .tasks
                    .get(&tid)
                    .map(|task| (task.cell_id.0, task.cell_generation))
                    .expect("service owner installed")
            })
        };
        crate::memory::cell_quota::register(QUOTA_CELL, 0x2345_0000);
        crate::loader::commit_launch_routes(
            ROUTE_TID,
            crate::task::LaunchRoutes {
                block_io: true,
                input: false,
                development_silo: false,
            },
        );
        assert_eq!(
            crate::cell::service_registry::register_as(
                crate::cell::service_registry::Registrar::Namespace,
                VFS_SERVICE_ID,
                VFS_TID,
                vfs_cell,
                vfs_generation,
                api::syscall::service_flags::ALL,
            ),
            Ok(())
        );
        crate::dev::driver_cell::set_input_cell(INPUT_TID);
        assert!(crate::cell::service_registry::register(
            SERVICE_ID,
            SERVICE_TID,
            service_cell,
            service_generation,
        ));
        assert!(crate::cell::hotswap::freeze_task_with_ceiling(REPLACEMENT_TID, 0xfeed).is_ok());
        Self {
            next_generation,
            block_io_registered,
            input_tid,
        }
    }

    fn owners_are_populated(&self) -> bool {
        let tasks_present = {
            let scheduler = crate::task::SCHEDULER.lock();
            let sched = scheduler.as_ref().expect("atomic corpus needs scheduler");
            [
                (VFS_TID, VFS_CELL),
                (INPUT_TID, INPUT_CELL),
                (SERVICE_TID, SERVICE_CELL),
                (REPLACEMENT_TID, REPLACEMENT_CELL),
                (QUOTA_TID, QUOTA_CELL),
                (ROUTE_TID, ROUTE_CELL),
            ]
            .iter()
            .all(|&(tid, cell)| {
                sched
                    .tasks
                    .get(&tid)
                    .is_some_and(|task| task.cell_id == cell)
            })
        };
        let replacement_present = crate::cell::hotswap::replacement_ceiling_snapshot()
            .iter()
            .any(|&(tid, ceiling)| tid == REPLACEMENT_TID && ceiling.is_some());
        tasks_present
            && replacement_present
            && crate::memory::cell_quota::registered_limit_for_test(QUOTA_CELL) == Some(0x2345_0000)
            && crate::loader::block_io_registered_snapshot()
            && crate::dev::driver_cell::input_cell_snapshot() == INPUT_TID
            && crate::cell::service_registry::snapshot()
                .iter()
                .any(|&(service, tid, _, flags)| {
                    service == VFS_SERVICE_ID
                        && tid == VFS_TID
                        && flags == api::syscall::service_flags::ALL
                })
            && crate::cell::service_registry::snapshot()
                .iter()
                .any(|&(service, tid, _, _)| service == SERVICE_ID && tid == SERVICE_TID)
    }
}

impl Drop for SharedStateBaseline {
    fn drop(&mut self) {
        crate::cell::hotswap::clear_swap_ceiling(REPLACEMENT_TID);
        crate::cell::service_registry::clear_tid(SERVICE_TID);
        crate::cell::service_registry::clear_tid(VFS_TID);
        crate::dev::driver_cell::clear_input_cell_if(INPUT_TID);
        if self.input_tid != 0 {
            crate::dev::driver_cell::set_input_cell(self.input_tid);
        }
        crate::loader::restore_block_io_registration_for_test(self.block_io_registered);
        crate::memory::cell_quota::deregister(QUOTA_CELL);
        let mut scheduler = crate::task::SCHEDULER.lock();
        let sched = scheduler.as_mut().expect("atomic corpus needs scheduler");
        for tid in [
            VFS_TID,
            INPUT_TID,
            SERVICE_TID,
            REPLACEMENT_TID,
            QUOTA_TID,
            ROUTE_TID,
        ] {
            assert!(sched.tasks.remove(&tid).is_some());
        }
        drop(scheduler);
        crate::task::tcb::restore_cell_generation_for_test(self.next_generation);
    }
}
