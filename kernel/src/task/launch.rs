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
    #[cfg(feature = "test-hooks")]
    pub development_silo: bool,
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
    #[cfg(cellos_p01_trace)]
    pub(crate) trace: Option<crate::p01_measurement::Attempt>,
    #[cfg(cellos_p01_trace)]
    pub(crate) bootstrap_init: bool,
    #[allow(dead_code)]
    pub(crate) is_domain: bool,
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
        is_domain: bool,
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
            #[cfg(cellos_p01_trace)]
            trace: None,
            #[cfg(cellos_p01_trace)]
            bootstrap_init: false,
            is_domain,
        }
    }
}

fn authority_is_current(
    sched: &super::scheduler::Scheduler,
    authority: &CallerLaunchAuthority,
) -> bool {
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
    // Phase-01 containment preflight (ADR-0019 §2.1): a live grant record that
    // names a private-root owner or receiver cannot be drained by this kernel, so
    // a domain-class publication is refused while one exists. This runs before the
    // scheduler lock is taken because the grant tables are only ever acquired in
    // the documented `*_GRANT_TABLE → SCHEDULER` order.
    #[cfg(feature = "native-domains")]
    if state.is_domain {
        crate::loader::domain_admission::refuse_while_domain_grant_live()?;
    }
    let mut scheduler = super::SCHEDULER.lock();
    let sched = scheduler.as_mut().ok_or(ViError::Unknown)?;
    let tid = sched.next_task_id;
    let requested_cell_id = prepared.requested_cell_id();
    if requested_cell_id.0 != 0
        && (requested_cell_id.0 as usize) >= crate::memory::cell_quota::MAX_CELLS
    {
        return Err(ViError::PermissionDenied);
    }
    if state
        .caller
        .as_ref()
        .is_some_and(|a| !authority_is_current(sched, a))
    {
        return Err(ViError::PermissionDenied);
    }
    // AP-11 must be injectable at the binding boundary even when this boot
    // corpus has no frozen replacement source to invalidate.
    crate::loader::atomic_checkpoint("AP-11")?;
    if state
        .replacement
        .as_ref()
        .is_some_and(|r| !r.can_bind(sched))
    {
        return Err(ViError::PermissionDenied);
    }

    let quota = if requested_cell_id == CellId(0) {
        crate::memory::cell_quota::QuotaReservation::reserve_next(state.quota_limit)?
    } else {
        crate::memory::cell_quota::QuotaReservation::reserve(requested_cell_id, state.quota_limit)?
    };
    let cell_id = quota.cell_id();
    if !sched.cell_owner_slot_is_empty(cell_id) {
        return Err(ViError::AlreadyExists);
    }
    crate::loader::atomic_checkpoint("AP-07")?;
    crate::loader::atomic_checkpoint("AP-08")?;
    let _kernel_alloc = super::scheduler::KernelAllocationContext::enter();

    let inherited_dirs = super::dir_inherit::take_for_launch(sched, state.inherit_from);
    // InheritedDirHandles is bounded inline state, not an independently
    // allocated buffer: its storage is already part of the task allocation.
    let (mut task, load_base) = prepared.into_task(tid, cell_id)?;
    task.inherited_dirs = inherited_dirs;
    // The command line lives on the task record itself, so it is reachable by
    // this task's own `StateRestore` and by nothing else, and it is installed
    // before the task can ever be selected.
    task.inherited_argv = state.argv.take();
    #[cfg(cellos_p01_trace)]
    crate::memory::heap::p01_owner::adopt_task(&task);
    task.syscall_allowlist = state.syscall_allowlist;
    task.cluster_mode = state.cluster_mode;
    task.cluster_id = state.cluster_id;
    task.priority = state.priority;
    task.pku_key = state.pku_key;
    task.pku_value = state.pku_value;
    task.is_critical = state.is_critical;
    task.root_tid = tid;
    let owner = api::cell_owner::CellOwner::new(cell_id.0, task.cell_generation, tid as u64);
    state.granted.apply_to(&mut task);
    #[cfg(feature = "test-hooks")]
    {
        task.development_silo_registration_cap = state
            .routes
            .development_silo
            .then(super::cap::DevelopmentSiloRegistrationCap::new);
    }
    if let Some(platform) = state.platform.take() {
        platform.commit_into(&mut task);
    }
    if let Some(replacement) = state.replacement.take() {
        replacement.commit_into(&mut task);
    }
    #[cfg(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    ))]
    if state.is_domain {
        // ADR-0019 §2.1 — the single on-path admission control. The lease is
        // taken before anything is created and re-checked immediately before the
        // task becomes runnable, so a concurrent drain either linearizes before
        // this admission or is observed by it. A denial is final: no task, no
        // domain, and no SAS fallback.
        let path = state.measurement.as_ref().map_or("", |measurement| measurement.path.as_str());
        let lease = crate::loader::domain_admission::admit_for_launch(path, state.granted)?;
        if let (Some(ks), Some(us), Some(seg)) =
            (&task.kernel_stack, &task.user_stack, &task.segment_mem)
        {
            let domain = crate::memory::address_space::create_cell_domain(ks, us, seg,
                #[cfg(cellos_p01_trace)] crate::p01_resources::owner(&task))
                .map_err(|_| ViError::OutOfMemory)?;
            if !lease.remains_enabled() {
                return Err(crate::loader::domain_admission::drain_refusal());
            }
            #[cfg(target_arch = "riscv64")]
            let arch_tag = "SATP";
            #[cfg(target_arch = "aarch64")]
            let arch_tag = "TTBR0";
            #[cfg(target_arch = "x86_64")]
            let arch_tag = "CR3";
            log::warn!(
                "[domain] admitted cell '{}' to Tier 2 Paged Domain ({} isolation)",
                task.name,
                arch_tag
            );
            task.bind_address_space(domain);
        } else {
            log::error!(
                "[domain] cannot create Tier 2 domain for '{}': missing stacks or segments",
                task.name
            );
            return Err(ViError::InvalidInput);
        }
    }
    #[cfg(not(all(
        feature = "native-domains",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    )))]
    if state.is_domain {
        log::error!(
            "[domain] Tier 2 Native Domain requested for '{}' but unsupported on this architecture/configuration",
            task.name
        );
        return Err(ViError::NotSupported);
    }

    #[cfg(cellos_p01_trace)]
    crate::memory::heap::p01_owner::task_backing(&task);
    // Private policy selection is tied to the measured artifact and the live
    // kernel-bootstrap publisher, never a caller-selected budget or task name.
    #[cfg(cellos_g1_cpu)]
    if let Some(measurement) = state.measurement.as_ref() {
        if measurement.path == "/bin/bench" {
            let attempt = state.trace.ok_or(ViError::PermissionDenied)?;
            let caller = state.caller.as_ref().ok_or(ViError::PermissionDenied)?;
            let publisher = sched.tasks.get(&caller.tid).ok_or(ViError::PermissionDenied)?;
            if !attempt.selected()
                || attempt.parent != crate::p01_measurement::Owner::of(publisher)
                || caller.generation != publisher.cell_generation
                || attempt.parent.tid != attempt.parent.root
                || !matches!(attempt.route, 1 | 2)
                || measurement.digest.as_slice() != &crate::p01_measurement::MANIFEST[32..64]
                || crate::p01_measurement::selected_owner(
                    publisher.cell_id.0, publisher.cell_generation).is_some()
            {
                return Err(ViError::PermissionDenied);
            }
            super::cpu_accounting::bind(sched, owner).map_err(|_| ViError::PermissionDenied)?;
        }
    }

    // Scheduler maps/route/audit storage are kernel bookkeeping, not child
    // objects merely because they are allocated during a selected launch.
    #[cfg(cellos_p01_trace)]
    let _heap_bookkeeping = crate::memory::heap::p01_owner::bookkeeping();
    {
        #[cfg(cellos_p01_trace)]
        let _semantic = crate::memory::heap::p01_owner::creation(None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED);
        sched.tasks.insert(tid, task);
    }
    assert!(
        sched.publish_live_cell_owner(owner),
        "reserved CellId owner slot must be empty"
    );
    sched.next_task_id = tid.checked_add(1).expect("task id space exhausted");
    quota.commit();
    crate::loader::commit_launch_routes(tid, cell_id, state.routes);
    #[cfg(cellos_p01_trace)]
    {
        let task = sched.tasks.get_mut(&tid).expect("committed publication");
        let diagnostic_owner = crate::p01_measurement::Owner::of(task);
        if let Some(measurement) = state.measurement.as_ref() {
            if state.bootstrap_init {
                crate::p01_measurement::arm(diagnostic_owner, measurement.digest);
            } else {
                crate::p01_measurement::publication(diagnostic_owner, state.trace, measurement.digest);
            }
        }
        crate::memory::heap::p01_owner::bind(state.trace, diagnostic_owner);
        crate::p01_resources::published(task);
    }
    if let Some(measurement) = state.measurement.take() {
        crate::measurement_log::commit_staged(tid, measurement.path, measurement.digest);
    }
    crate::audit::log_event(
        crate::audit::AuditEvent::CellSpawn,
        &crate::audit::encode_u32x2(tid as u32, 0),
    );
    crate::loader::observe_pre_ready(sched, tid);
    sched.push_ready(tid);
    Ok((tid, load_base))
}

/// Creator-quota claim for resources a prepared child will retain or briefly
/// occupy. It is charged to the creator's exact live Cell before preparation
/// succeeds and refunded exactly once when the preparation is dropped; a
/// successful commit transfers the retained portion (the historical measurement
/// entry and its path) to the retained log owner instead of refunding it.
struct PublicationClaim {
    cell: usize,
    retained: usize,
    transient: usize,
    armed: bool,
}
impl PublicationClaim {
    fn charge(cell: usize, retained: usize, transient: usize) -> Result<Self, ViError> {
        let total = retained.checked_add(transient).ok_or(ViError::OutOfMemory)?;
        if !crate::memory::cell_quota::charge(cell, total) {
            return Err(ViError::OutOfMemory);
        }
        Ok(Self { cell, retained, transient, armed: true })
    }
    /// Commit: the queue slot is consumed by the runnable task, so only that
    /// transient part is refunded; the retained log history stays attributed.
    fn transfer(mut self) {
        crate::memory::cell_quota::refund(self.cell, self.transient);
        self.armed = false;
    }
}
impl Drop for PublicationClaim {
    fn drop(&mut self) {
        if self.armed {
            crate::memory::cell_quota::refund(self.cell, self.retained + self.transient);
        }
    }
}

/// The task, quota, CSpace, measurement-log and ready-queue slots remain
/// detached promises until the single session owner transaction commits. This
/// is not a second live-owner table: the Cell owner slot is `Reserved`, which
/// no resolve/watch path can observe as live.
pub(crate) struct SessionPublication {
    pub(crate) task: alloc::boxed::Box<super::Task>,
    pub(crate) owner: api::cell_owner::CellOwner,
    state: TaskLaunchState,
    quota: crate::memory::cell_quota::QuotaReservation,
    authority: crate::cell::cspace::AuthorityReservation,
    measurement: crate::measurement_log::PublicationReservation,
    ready: super::hart_local::ready::PublicationReservation,
    claim: PublicationClaim,
}
impl SessionPublication {
    /// Every fallible allocation, quota, owner-slot, log and ready-queue claim
    /// happens here; `publish` allocates nothing and cannot fail.
    pub(crate) fn reserve(sched: &mut super::scheduler::Scheduler,
        prepared: super::PreparedElfTask, state: TaskLaunchState) -> Result<Self, ViError> {
        let Some(caller) = state.caller.as_ref() else { return Err(ViError::PermissionDenied); };
        if !authority_is_current(sched, caller) {
            return Err(ViError::PermissionDenied);
        }
        let (creator, creator_generation) = sched.tasks.get(&caller.tid)
            .map(|t| (t.cell_id.0 as usize, t.cell_generation))
            .ok_or(ViError::PermissionDenied)?;
        // A session child is an ordinary governed path launch: never a boot
        // singleton, domain, hotswap replacement, or service route publisher.
        if prepared.requested_cell_id() != CellId(0) || state.is_domain || state.platform.is_some()
            || state.replacement.is_some() || state.routes.block_io || state.routes.input {
            return Err(ViError::PermissionDenied);
        }
        #[cfg(feature = "test-hooks")]
        if state.routes.development_silo { return Err(ViError::PermissionDenied); }
        // Heap bookkeeping for the unpublished task is kernel memory, exactly as
        // in `publish_prepared`; per-principal accounting is the explicit claim.
        let _kernel_alloc = super::scheduler::KernelAllocationContext::enter();
        let tid = sched.next_task_id;
        let next = tid.checked_add(1).ok_or(ViError::OutOfMemory)?;
        let quota = crate::memory::cell_quota::QuotaReservation::reserve_next(state.quota_limit)?;
        let cell = quota.cell_id();
        let (mut task, _) = prepared.into_task(tid, cell)?;
        task.root_tid = tid;
        task.syscall_allowlist = state.syscall_allowlist;
        task.cluster_mode = state.cluster_mode;
        task.cluster_id = state.cluster_id;
        task.priority = state.priority;
        task.pku_key = state.pku_key;
        task.pku_value = state.pku_value;
        task.is_critical = state.is_critical;
        state.granted.apply_to(&mut task);
        #[cfg(feature = "test-hooks")]
        { task.development_silo_registration_cap = state.routes.development_silo
            .then(super::cap::DevelopmentSiloRegistrationCap::new); }
        let owner = api::cell_owner::CellOwner::new(cell.0, task.cell_generation, tid as u64);
        let authority = crate::cell::cspace::AuthorityReservation::reserve(cell, owner.generation, state.granted)?;
        let path_len = state.measurement.as_ref().map_or(0, |m| m.path.len());
        let claim = PublicationClaim::charge(creator,
            crate::measurement_log::PublicationReservation::ENTRY_BYTES + path_len,
            super::hart_local::ready::PublicationReservation::SLOT_BYTES)?;
        let measurement = crate::measurement_log::PublicationReservation::reserve(
            (creator as u64, creator_generation))?;
        let ready = super::hart_local::ready::PublicationReservation::reserve(state.priority)?;
        sched.reserve_session_owner(owner)?;
        sched.next_task_id = next;
        Ok(Self { task, owner, state, quota, authority, measurement, ready, claim })
    }
    pub(crate) fn current(&self, sched: &super::scheduler::Scheduler) -> bool {
        if !sched.session_owner_reserved(self.owner)
            || self.state.caller.as_ref().is_none_or(|a| !authority_is_current(sched, a)) {
            return false;
        }
        let Some(caller) = self.state.caller.as_ref().and_then(|a| sched.tasks.get(&a.tid)) else { return false; };
        let Some(measurement) = self.state.measurement.as_ref() else { return false; };
        let state = crate::loader::launch_profile::CallerLaunchState { name: &caller.name,
            has_spawn: caller.spawn_cap.is_some(), has_supervisor: caller.supervisor_cap.is_some() };
        crate::loader::launch_profile::authorize(state, crate::loader::launch_profile::LaunchRoute::Path,
            &measurement.path).is_some_and(|profile| self.state.granted.intersect(profile.child_ceiling)
                == self.state.granted && (!profile.requires_lifecycle_authority || caller.spawn_cap.is_some()))
    }
    /// Final publication. Called with every owner check already passed, under
    /// SCHEDULER, and performs no allocation: capacity for the task row, CSpace
    /// row, log entry and ready entry was promised in `reserve`.
    pub(crate) fn publish(mut self, sched: &mut super::scheduler::Scheduler) {
        let tid = self.owner.root_tid as usize;
        #[cfg(cellos_p01_trace)]
        {
            crate::memory::heap::p01_owner::adopt_task(&self.task);
            crate::memory::heap::p01_owner::task_backing(&self.task);
        }
        self.authority.commit();
        sched.tasks.publish_reserved(tid, self.task);
        sched.publish_session_owner(self.owner);
        self.quota.commit();
        crate::loader::commit_launch_routes(tid, CellId(self.owner.cell_id), self.state.routes);
        #[cfg(cellos_p01_trace)]
        {
            let task = sched.tasks.get_mut(&tid).expect("committed publication");
            let diagnostic_owner = crate::p01_measurement::Owner::of(task);
            if let Some(measurement) = self.state.measurement.as_ref() {
                if self.state.bootstrap_init {
                    crate::p01_measurement::arm(diagnostic_owner, measurement.digest);
                } else {
                    crate::p01_measurement::publication(diagnostic_owner, self.state.trace, measurement.digest);
                }
            }
            crate::memory::heap::p01_owner::bind(self.state.trace, diagnostic_owner);
            crate::p01_resources::published(task);
        }
        if let Some(measurement) = self.state.measurement.take() {
            self.measurement.commit(tid, measurement.path, measurement.digest);
        }
        crate::audit::log_event(
            crate::audit::AuditEvent::CellSpawn,
            &crate::audit::encode_u32x2(tid as u32, 0),
        );
        crate::loader::observe_pre_ready(sched, tid);
        // The notification follows the reservation's pinned destination, not
        // whichever hart happens to commit or whatever RT hart is online now.
        let (hart, priority) = self.ready.destination();
        self.ready.commit(tid);
        self.claim.transfer();
        sched.pend_preempt_on_hart(hart, priority);
    }
}
