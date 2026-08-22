//! Complete launch state and the sole ELF scheduler publication point.

use alloc::string::String;
use alloc::vec::Vec;
use types::{CellId, ViError};

pub struct CallerLaunchAuthority {
    pub tid: usize,
    pub generation: u64,
    pub ceiling: super::cap::CapSet,
}

#[derive(Clone, Copy)]
pub struct LaunchRoutes {
    pub block_io: bool,
    pub input: bool,
}

pub struct StagedMeasurement {
    pub path: String,
    pub digest: [u8; 32],
}

/// Security-complete data and reservations for one unpublished ELF task.
pub struct TaskLaunchState {
    pub(crate) caller: Option<CallerLaunchAuthority>,
    pub(crate) granted: super::cap::CapSet,
    pub(crate) platform: Option<super::cap::PlatformCapReservation>,
    pub(crate) replacement: Option<crate::cell::hotswap::ReplacementReservation>,
    pub(crate) quota_limit: usize,
    pub(crate) syscall_allowlist: u64,
    pub(crate) cluster_mode: u8,
    pub(crate) cluster_id: u64,
    pub(crate) priority: u8,
    pub(crate) pku_key: u8,
    pub(crate) pku_value: u32,
    pub(crate) is_critical: bool,
    pub(crate) inherit_from: usize,
    pub(crate) argv: Option<Vec<u8>>,
    pub(crate) routes: LaunchRoutes,
    pub(crate) measurement: Option<StagedMeasurement>,
}

impl TaskLaunchState {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn complete(
        caller: Option<CallerLaunchAuthority>,
        granted: super::cap::CapSet,
        platform: Option<super::cap::PlatformCapReservation>,
        replacement: Option<crate::cell::hotswap::ReplacementReservation>,
        quota_limit: usize,
        syscall_allowlist: u64,
        cluster_mode: u8,
        cluster_id: u64,
        priority: u8,
        pku_key: u8,
        pku_value: u32,
        is_critical: bool,
        inherit_from: usize,
        argv: Option<Vec<u8>>,
        routes: LaunchRoutes,
        measurement: Option<StagedMeasurement>,
    ) -> Self {
        Self {
            caller,
            granted,
            platform,
            replacement,
            quota_limit,
            syscall_allowlist,
            cluster_mode,
            cluster_id,
            priority,
            pku_key,
            pku_value,
            is_critical,
            inherit_from,
            argv,
            routes,
            measurement,
        }
    }
}

fn authority_is_current(sched: &super::scheduler::Scheduler, authority: &CallerLaunchAuthority) -> bool {
    sched.tasks.get(&authority.tid).is_some_and(|task| {
        task.cell_generation == authority.generation
            && super::cap::CapSet::of_task(task).intersect(authority.ceiling) == authority.ceiling
    })
}

/// Publish a fully prepared ELF task. Every fallible check precedes mutation;
/// the final ready-queue push is the only runnable publication point.
pub fn publish_prepared(
    prepared: super::PreparedElfTask,
    mut state: TaskLaunchState,
) -> Result<(usize, usize), ViError> {
    let mut scheduler = super::SCHEDULER.lock();
    let sched = scheduler.as_mut().ok_or(ViError::Unknown)?;
    let tid = sched.next_task_id;
    let requested_cell_id = prepared.requested_cell_id();
    if requested_cell_id.0 != 0
        && (requested_cell_id.0 as usize) >= crate::memory::cell_quota::MAX_CELLS
    {
        return Err(ViError::PermissionDenied);
    }
    if state.caller.as_ref().is_some_and(|a| !authority_is_current(sched, a)) {
        return Err(ViError::PermissionDenied);
    }
    // AP-11 must be injectable at the binding boundary even when this boot
    // corpus has no frozen replacement source to invalidate.
    crate::loader::atomic_checkpoint("AP-11")?;
    if state.replacement.as_ref().is_some_and(|r| !r.can_bind(sched)) {
        return Err(ViError::PermissionDenied);
    }

    let quota = if requested_cell_id == CellId(0) {
        crate::memory::cell_quota::QuotaReservation::reserve_next(state.quota_limit)?
    } else {
        crate::memory::cell_quota::QuotaReservation::reserve(requested_cell_id, state.quota_limit)?
    };
    let cell_id = quota.cell_id();
    crate::loader::atomic_checkpoint("AP-07")?;
    crate::loader::atomic_checkpoint("AP-08")?;
    let accounting_cell = super::hart_local::current_cell_id();
    super::hart_local::set_current_cell_id(0);

    let inherited_dirs = super::dir_inherit::take_for_launch(sched, state.inherit_from);
    let (mut task, load_base) = prepared.into_task(tid, cell_id);
    task.inherited_dirs = inherited_dirs;
    task.syscall_allowlist = state.syscall_allowlist;
    task.cluster_mode = state.cluster_mode;
    task.cluster_id = state.cluster_id;
    task.priority = state.priority;
    task.pku_key = state.pku_key;
    task.pku_value = state.pku_value;
    task.is_critical = state.is_critical;
    state.granted.apply_to(&mut task);
    if let Some(platform) = state.platform.take() {
        platform.commit_into(&mut task);
    }
    if let Some(replacement) = state.replacement.take() {
        replacement.commit_into(&mut task);
    }

    sched.tasks.insert(tid, task);
    sched.next_task_id = tid.checked_add(1).expect("task id space exhausted");
    quota.commit();
    crate::loader::commit_launch_routes(tid, cell_id, state.routes);
    if let Some(argv) = state.argv.take() {
        crate::cell::state_stash::install_spawn_argv(tid, argv);
    }
    if let Some(measurement) = state.measurement.take() {
        crate::measurement_log::commit_staged(tid, measurement.path, measurement.digest);
    }
    crate::audit::log_event(
        crate::audit::AuditEvent::CellSpawn,
        &crate::audit::encode_u32x2(tid as u32, 0),
    );
    super::hart_local::set_current_cell_id(accounting_cell);
    crate::loader::observe_pre_ready(sched, tid);
    sched.push_ready(tid);
    Ok((tid, load_base))
}
