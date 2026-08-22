use core::sync::atomic::Ordering;
use types::ViError;

use super::harness::{arm_failure, failure_reached};

pub(super) const AUDIT_RECORD_BYTES: usize = 18;


#[derive(Debug, PartialEq)]
pub(super) struct TaskSnapshot {
    pub(super) id: usize,
    cell_id: u64,
    generation: u64,
    state: crate::task::tcb::TaskState,
    priority: u8,
    cluster: (u8, u64),
    allowlist: u64,
    protection: (u8, u32),
    caps: crate::task::cap::CapSet,
    critical: bool,
    replacement: Option<usize>,
    dirs: (u64, u64, usize),
    mappings: alloc::vec::Vec<(usize, usize, usize, u64, crate::memory::vma::VmaKind)>,
}

#[derive(Debug, PartialEq)]
pub(super) struct StateSnapshot {
    pub(super) tasks: alloc::vec::Vec<TaskSnapshot>,
    pub(super) zombies: alloc::vec::Vec<(usize, u64, crate::task::tcb::TaskState)>,
    pub(super) next_task_id: usize,
    scheduler_counters: (usize, usize, usize, usize),
    current: alloc::vec::Vec<(usize, usize, usize)>,
    pub(super) ready: alloc::vec::Vec<
        alloc::collections::BTreeMap<u8, alloc::collections::VecDeque<usize>>,
    >,
    free_frames: Option<usize>,
    pub(super) quota: crate::memory::cell_quota::QuotaSnapshot,
    platform: bool,
    va: (usize, alloc::vec::Vec<u64>),
    replacement: alloc::vec::Vec<(usize, Option<(crate::task::cap::CapSet, u64, u64, u64)>)>,
    block_io_registered: bool,
    vfs_tid: usize,
    vfs_handler: usize,
    input_tid: usize,
    service_routes: alloc::vec::Vec<(u16, usize, bool)>,
    argv_and_stash: alloc::vec::Vec<(u64, alloc::vec::Vec<u8>)>,
    pub(super) measurements: (usize, [u8; 32]),
    pub(super) audit: (usize, usize, usize),
}



pub(super) fn snapshot() -> StateSnapshot {
    let (tasks, zombies, next_task_id, scheduler_counters) = {
        let scheduler = crate::task::SCHEDULER.lock();
        let sched = scheduler.as_ref().expect("atomic corpus needs scheduler");
        (
            sched.tasks.values().map(|task| TaskSnapshot {
                id: task.id,
                cell_id: task.cell_id.0,
                generation: task.cell_generation,
                state: task.state.clone(),
                priority: task.priority,
                cluster: (task.cluster_mode, task.cluster_id),
                allowlist: task.syscall_allowlist,
                protection: (task.pku_key, task.pku_value),
                caps: crate::task::cap::CapSet::of_task(task),
                critical: task.is_critical,
                replacement: task.hotswap_source_tid,
                dirs: (
                    task.inherited_dirs.spawner_cell_id,
                    task.inherited_dirs.spawner_generation,
                    task.inherited_dirs.set.len(),
                ),
                mappings: task.vma.0.iter().map(|mapping| (
                    mapping.va_start,
                    mapping.va_end,
                    mapping.pa_start,
                    mapping.flags,
                    mapping.kind.clone(),
                )).collect(),
            }).collect(),
            sched.zombies.iter().map(|task| (
                task.id, task.cell_id.0, task.state.clone(),
            )).collect(),
            sched.next_task_id,
            sched.publication_snapshot_counters(),
        )
    };
    let current = crate::task::hart_local::HART_LOCALS.iter().map(|hart| (
        hart.hart_id,
        hart.current_task_id.load(Ordering::Acquire),
        hart.current_cell_id.load(Ordering::Acquire),
    )).collect();
    let ready = crate::task::hart_local::HART_LOCALS
        .iter()
        .map(|hart| hart.ready.lock().clone())
        .collect();
    let free_frames = crate::memory::frame::FRAME_ALLOCATOR
        .lock().as_ref().map(|allocator| allocator.free_frames());
    StateSnapshot {
        tasks,
        zombies,
        next_task_id,
        scheduler_counters,
        current,
        ready,
        free_frames,
        quota: crate::memory::cell_quota::snapshot(),
        platform: crate::task::cap::platform_reserved_or_committed(),
        va: super::super::va_alloc::snapshot(),
        replacement: crate::cell::hotswap::replacement_ceiling_snapshot(),
        block_io_registered: crate::loader::block_io_registered_snapshot(),
        vfs_tid: crate::fast_ipc::vfs_handler_cell_snapshot(),
        vfs_handler: crate::fast_ipc::vfs_handler_pointer_snapshot(),
        input_tid: crate::task::drivers::driver_cell::input_cell_snapshot(),
        service_routes: crate::cell::service_registry::snapshot(),
        argv_and_stash: crate::cell::state_stash::snapshot(),
        audit: crate::audit::snapshot(),
        measurements: (
            crate::measurement_log::entry_count(),
            crate::measurement_log::aggregate(),
        ),
    }
}

/// Denial helpers prove the documented audit-record count. Fixture teardown may
/// compare state only after proving that those records remain append-only
/// evidence and the fixture itself emitted nothing.
pub(super) fn exclude_proven_audit_delta(
    expected: &StateSnapshot,
    before_teardown: &StateSnapshot,
    observed: &mut StateSnapshot,
    audit_records: usize,
) -> bool {
    let expected_audit = (
        expected
            .audit
            .0
            .wrapping_add(AUDIT_RECORD_BYTES.wrapping_mul(audit_records)),
        expected.audit.1,
        expected.audit.2,
    );
    let verified = before_teardown.audit == expected_audit
        && observed.audit == before_teardown.audit;
    if verified {
        observed.audit = expected.audit;
    }
    verified
}

pub(super) fn snapshot_matches(
    case: &'static str,
    stage: &'static str,
    before: &StateSnapshot,
    after: &StateSnapshot,
) -> bool {
    if before == after {
        return true;
    }
    log::error!(
        "ATOMIC_PUBLICATION_{}: MISMATCH stage={} field=state",
        case,
        stage,
    );
    report_task_mismatches(case, stage, &before.tasks, &after.tasks);
    report_mismatch(case, stage, "zombies", before.zombies != after.zombies);
    report_mismatch(case, stage, "next-task-id", before.next_task_id != after.next_task_id);
    report_mismatch(
        case,
        stage,
        "scheduler-counters",
        before.scheduler_counters != after.scheduler_counters,
    );
    report_mismatch(case, stage, "hart-current", before.current != after.current);
    report_mismatch(case, stage, "ready-queues", before.ready != after.ready);
    report_mismatch(case, stage, "free-frames", before.free_frames != after.free_frames);
    report_mismatch(
        case,
        stage,
        "quota",
        before.quota != after.quota,
    );
    report_mismatch(case, stage, "platform-reservation", before.platform != after.platform);
    report_mismatch(case, stage, "cell-va", before.va != after.va);
    report_mismatch(
        case,
        stage,
        "replacement-ceilings",
        before.replacement != after.replacement,
    );
    report_mismatch(
        case,
        stage,
        "block-io-route",
        before.block_io_registered != after.block_io_registered,
    );
    report_mismatch(case, stage, "vfs-route", before.vfs_tid != after.vfs_tid);
    report_mismatch(
        case,
        stage,
        "vfs-handler",
        before.vfs_handler != after.vfs_handler,
    );
    report_mismatch(case, stage, "input-route", before.input_tid != after.input_tid);
    report_mismatch(
        case,
        stage,
        "service-routes",
        before.service_routes != after.service_routes,
    );
    report_mismatch(
        case,
        stage,
        "argv-stash",
        before.argv_and_stash != after.argv_and_stash,
    );
    report_mismatch(case, stage, "audit", before.audit != after.audit);
    report_mismatch(
        case,
        stage,
        "measurements",
        before.measurements != after.measurements,
    );
    false
}

fn report_mismatch(case: &'static str, stage: &'static str, field: &'static str, mismatched: bool) {
    if mismatched {
        log::error!(
            "ATOMIC_PUBLICATION_{}: MISMATCH stage={} field={}",
            case,
            stage,
            field,
        );
    }
}

fn report_task_mismatches(
    case: &'static str,
    stage: &'static str,
    before: &[TaskSnapshot],
    after: &[TaskSnapshot],
) {
    let shared = before.len().min(after.len());
    for index in 0..shared {
        let expected = &before[index];
        let actual = &after[index];
        if expected.id != actual.id {
            log::error!(
                "ATOMIC_PUBLICATION_{}: MISMATCH stage={} resource=task-order",
                case,
                stage,
            );
            return;
        }
        report_task_field(case, stage, expected.id, "cell-id", expected.cell_id != actual.cell_id);
        report_task_field(case, stage, expected.id, "generation", expected.generation != actual.generation);
        report_task_field(case, stage, expected.id, "state", expected.state != actual.state);
        report_task_field(case, stage, expected.id, "priority", expected.priority != actual.priority);
        report_task_field(case, stage, expected.id, "cluster", expected.cluster != actual.cluster);
        report_task_field(case, stage, expected.id, "allowlist", expected.allowlist != actual.allowlist);
        report_task_field(case, stage, expected.id, "protection", expected.protection != actual.protection);
        report_task_field(case, stage, expected.id, "caps", expected.caps != actual.caps);
        report_task_field(case, stage, expected.id, "critical", expected.critical != actual.critical);
        report_task_field(
            case,
            stage,
            expected.id,
            "replacement-source",
            expected.replacement != actual.replacement,
        );
        report_task_field(case, stage, expected.id, "dirs", expected.dirs != actual.dirs);
        report_task_field(case, stage, expected.id, "mappings", expected.mappings != actual.mappings);
    }
    if before.len() != after.len() {
        log::error!(
            "ATOMIC_PUBLICATION_{}: MISMATCH stage={} resource=task-set",
            case,
            stage,
        );
    }
}

fn report_task_field(
    case: &'static str,
    stage: &'static str,
    tid: usize,
    field: &'static str,
    mismatched: bool,
) {
    if mismatched {
        log::error!(
            "ATOMIC_PUBLICATION_{}: MISMATCH stage={} resource=task field={} tid={}",
            case,
            stage,
            field,
            tid,
        );
    }
}

fn report_contract(case: &'static str, field: &'static str, satisfied: bool) {
    if !satisfied {
        log::error!("ATOMIC_PUBLICATION_{}: MISMATCH field={}", case, field);
    }
}

pub(super) fn denied_with_full_rollback(
    case: &'static str,
    action: impl FnOnce() -> Result<(), ViError>,
) -> bool {
    if case == "AP-02" {
        super::probe::arm_ap02();
    }
    let before = snapshot();
    arm_failure(case);
    let denied = action().is_err();
    let restored = snapshot_matches(case, "denial-rollback", &before, &snapshot());
    let unpublished_cleanup = case != "AP-02" || super::probe::ap02_cleanup_complete();
    let reached = failure_reached();
    report_contract(case, "denied", denied);
    report_contract(case, "checkpoint", reached);
    report_contract(case, "ap02-translation-cleanup", unpublished_cleanup);
    denied && reached && unpublished_cleanup && restored
}

/// Governed admission appends the verified-signature and privileged-grant audit
/// records before a test hook rejects publication. Both are evidence of the
/// normal signed test-hooks image flow, not transaction residue.
pub(super) fn denied_with_governed_rollback(
    case: &'static str,
    audit_records: usize,
    action: impl FnOnce() -> Result<(), ViError>,
) -> bool {
    let before = snapshot();
    arm_failure(case);
    let denied = action().is_err();
    let mut after = snapshot();
    let expected_audit = (
        before
            .audit
            .0
            .wrapping_add(AUDIT_RECORD_BYTES.wrapping_mul(audit_records)),
        before.audit.1,
        before.audit.2,
    );
    let audited = after.audit == expected_audit;
    report_contract(case, "governed-audit", audited);
    after.audit = before.audit;
    let restored = snapshot_matches(case, "governed-rollback", &before, &after);
    let reached = failure_reached();
    report_contract(case, "denied", denied);
    report_contract(case, "checkpoint", reached);
    denied && reached && audited && restored
}


/// A denial audit record is evidence of the rejected request, not publication
/// residue. Keep it out of the rollback comparison only after proving its
/// exact, fixed-size record was appended.
pub(super) fn denied_with_logged_denial(
    case: &'static str,
    action: impl FnOnce() -> Result<(), ViError>,
) -> bool {
    let before = snapshot();
    arm_failure(case);
    let denied = action().is_err();
    let mut after = snapshot();
    let expected_audit = (
        before.audit.0.wrapping_add(AUDIT_RECORD_BYTES),
        before.audit.1,
        before.audit.2,
    );
    let logged_denial = after.audit == expected_audit;
    report_contract(case, "denial-audit", logged_denial);
    after.audit = before.audit;
    let restored = snapshot_matches(case, "logged-denial-rollback", &before, &after);
    let reached = failure_reached();
    report_contract(case, "denied", denied);
    report_contract(case, "checkpoint", reached);
    denied && reached && logged_denial && restored
}


pub(super) fn platform_singleton_denial() -> bool {
    // The in-memory embedded ELF is admitted under the real /bin/platform path,
    // so AP-05 and AP-06 still traverse its ceiling, policy, and singleton flow.
    let injected = denied_with_governed_rollback("AP-05", 2, || {
        super::spawn_governed_platform(
            super::super::SpawnRequest::governed_boot(),
        ).map(|_| ())
    });
    let before = snapshot();
    let Ok(held_platform) = crate::task::cap::reserve_platform() else {
        report_contract("AP-05", "platform-hold", false);
        return false;
    };
    let held = snapshot();
    let denied = matches!(
        super::spawn_governed_platform(
            super::super::SpawnRequest::governed_boot(),
        ),
        Err(ViError::PermissionDenied)
    );
    let mut after = snapshot();
    let audited = after.audit == (
        held.audit.0.wrapping_add(AUDIT_RECORD_BYTES * 2),
        held.audit.1,
        held.audit.2,
    );
    report_contract("AP-05", "platform-singleton-audit", audited);
    after.audit = held.audit;
    let held_restored = snapshot_matches("AP-05", "platform-held-denial", &held, &after);
    drop(held_platform);
    let mut restored = snapshot();
    restored.audit = before.audit;
    let released = snapshot_matches("AP-05", "platform-release", &before, &restored);
    report_contract("AP-05", "platform-singleton-denied", denied);
    injected && denied && audited && held_restored && released
}
