//! Complete launch state and the sole ELF scheduler publication point.

use alloc::string::String;
use alloc::vec::Vec;
use types::{CellId, ViError};

pub struct CallerLaunchAuthority {
    pub tid: usize,
    pub cell_id: u64,
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
    /// Identity bound to the published root (`Task::bind_image`); its digest
    /// is `digest`, plus the signing anchor and vouched name if verified.
    pub image: abi::cell_image::ImageIdentity,
    /// The kernel resolved `path` itself and it may name its Cell
    /// (`catalog::names_launch_identity`); copied to `Task::launch_named`.
    pub launch_named: bool,
}

/// Security-complete data and reservations for one unpublished ELF task.
pub struct TaskLaunchState {
    pub(crate) caller: Option<CallerLaunchAuthority>,
    pub(crate) granted: super::cap::CapSet,
    pub(crate) platform: Option<super::cap::PlatformCapReservation>,
    pub(crate) replacement: Option<crate::cell::hotswap::ReplacementReservation>,
    pub(crate) quota_limit: usize,
    pub(crate) syscall_allowlist: u64,
    pub(crate) priority: u8,
    pub(crate) pku_key: u8,
    pub(crate) pku_value: u32,
    pub(crate) is_critical: bool,
    pub(crate) inherit_from: usize,
    pub(crate) argv: Option<Vec<u8>>,
    pub(crate) routes: LaunchRoutes,
    pub(crate) measurement: Option<StagedMeasurement>,
    #[cfg(feature = "p01-trace")]
    pub(crate) trace: Option<crate::p01_measurement::Attempt>,
    #[cfg(feature = "p01-trace")]
    pub(crate) bootstrap_init: bool,
    /// This is the kernel's trusted init: the latch is published before the
    /// task becomes runnable, so init never runs with an unnamed identity.
    pub(crate) trusted_init: bool,
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
            priority,
            pku_key,
            pku_value,
            is_critical,
            inherit_from,
            argv,
            routes,
            measurement,
            #[cfg(feature = "p01-trace")]
            trace: None,
            #[cfg(feature = "p01-trace")]
            bootstrap_init: false,
            trusted_init: false,
            is_domain,
        }
    }
}

fn authority_is_current(
    sched: &crate::sched::scheduler::Scheduler,
    authority: &CallerLaunchAuthority,
) -> bool {
    sched.tasks.get(&authority.tid).is_some_and(|task| {
        task.cell_generation == authority.generation
            && sched
                .cell_rights(authority.tid)
                .intersect(authority.ceiling)
                == authority.ceiling
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

    // The quota row carries the task's generation, so it is minted first.
    let generation = super::tcb::next_cell_generation();
    let limits = crate::memory::cell_quota::Limits {
        memory: state.quota_limit,
    };
    let quota = if requested_cell_id == CellId(0) {
        crate::memory::cell_quota::QuotaReservation::reserve_next(generation, limits)?
    } else {
        crate::memory::cell_quota::QuotaReservation::reserve(requested_cell_id, generation, limits)?
    };
    let cell_id = quota.cell_id();
    if !sched.cell_owner_slot_is_empty(cell_id) {
        return Err(ViError::AlreadyExists);
    }
    crate::loader::atomic_checkpoint("AP-07")?;
    crate::loader::atomic_checkpoint("AP-08")?;
    let _kernel_alloc = crate::sched::scheduler::KernelAllocationContext::enter();

    let inherited_dirs = super::dir_inherit::take_for_launch(sched, state.inherit_from);
    // InheritedDirHandles is bounded inline state, not an independently
    // allocated buffer: its storage is already part of the task allocation.
    let (mut task, load_base) = prepared.into_task(tid, cell_id, generation)?;
    task.inherited_dirs = inherited_dirs;
    // The command line lives on the task record itself, so it is reachable by
    // this task's own `StateRestore` and by nothing else, and it is installed
    // before the task can ever be selected.
    task.inherited_argv = state.argv.take();
    #[cfg(feature = "p01-trace")]
    crate::memory::heap::p01_owner::adopt_task(&task);
    task.syscall_allowlist = state.syscall_allowlist;
    task.priority = state.priority;
    task.pku_key = state.pku_key;
    task.pku_value = state.pku_value;
    task.is_critical = state.is_critical;
    task.root_tid = tid;
    task.spawner = state
        .caller
        .as_ref()
        .map(|caller| (caller.cell_id, caller.generation));
    if let Some(measurement) = state.measurement.as_ref() {
        // A fresh task from `into_task`, so the first binding always lands.
        task.bind_image(measurement.image);
        task.launch_named = measurement.launch_named;
    }
    let owner = abi::cell_owner::CellOwner::new(cell_id.0, task.cell_generation, tid as u64);
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
    if state.is_domain {
        // ADR-0019 §2.1: the one admission policy (see `admit_and_bind_domain`).
        let path = state
            .measurement
            .as_ref()
            .map_or("", |measurement| measurement.path.as_str());
        admit_and_bind_domain(&mut task, path, state.granted)?;
        log_domain_admitted(&task.name);
    }

    #[cfg(feature = "p01-trace")]
    crate::memory::heap::p01_owner::task_backing(&task);
    // Private policy selection is tied to the measured artifact and the live
    // kernel-bootstrap publisher, never a caller-selected budget or task name.
    #[cfg(feature = "g1-cpu-accounting")]
    if let Some(measurement) = state.measurement.as_ref() {
        if measurement.path == "/bin/bench" {
            let attempt = state.trace.ok_or(ViError::PermissionDenied)?;
            let caller = state.caller.as_ref().ok_or(ViError::PermissionDenied)?;
            let publisher = sched
                .tasks
                .get(&caller.tid)
                .ok_or(ViError::PermissionDenied)?;
            if !attempt.selected()
                || attempt.parent != crate::p01_measurement::Owner::of(publisher)
                || caller.generation != publisher.cell_generation
                || attempt.parent.tid != attempt.parent.root
                || !matches!(attempt.route, 1 | 2)
                || measurement.digest.as_slice() != &crate::p01_measurement::MANIFEST[32..64]
                || crate::p01_measurement::selected_owner(
                    publisher.cell_id.0,
                    publisher.cell_generation,
                )
                .is_some()
            {
                return Err(ViError::PermissionDenied);
            }
            crate::sched::cpu_accounting::bind(sched, owner)
                .map_err(|_| ViError::PermissionDenied)?;
        }
    }

    // Scheduler maps/route/audit storage are kernel bookkeeping, not child
    // objects merely because they are allocated during a selected launch.
    #[cfg(feature = "p01-trace")]
    let _heap_bookkeeping = crate::memory::heap::p01_owner::bookkeeping();
    {
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED,
        );
        sched.tasks.insert(tid, task);
    }
    assert!(
        sched.publish_live_cell_owner(owner),
        "reserved CellId owner slot must be empty"
    );
    sched.next_task_id = tid.checked_add(1).expect("task id space exhausted");
    quota.commit();
    crate::loader::commit_launch_routes(tid, state.routes);
    #[cfg(feature = "p01-trace")]
    {
        let task = sched.tasks.get_mut(&tid).expect("committed publication");
        let diagnostic_owner = crate::p01_measurement::Owner::of(task);
        if let Some(measurement) = state.measurement.as_ref() {
            if state.bootstrap_init {
                crate::p01_measurement::arm(diagnostic_owner, measurement.digest);
            } else {
                crate::p01_measurement::publication(
                    diagnostic_owner,
                    state.trace,
                    measurement.digest,
                );
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
    if state.trusted_init {
        // Init's tid keys its launch identity and the `MACHINE_PIN`
        // registration restriction (ADR-0029 §3); publish before it can run.
        crate::task::syscall::publish_trusted_init(tid);
    }
    sched.push_ready(tid);
    Ok((tid, load_base))
}

/// Creator-quota claim for resources a prepared child will retain or briefly
/// occupy. It is charged to the creator's exact incarnation before preparation
/// succeeds and refunded exactly once when the preparation is dropped; a
/// successful commit keeps the retained portion (the historical measurement
/// entry and its path) charged to the creator instead of refunding it.
struct PublicationClaim {
    retained: crate::memory::cell_quota::Reservation,
    transient: crate::memory::cell_quota::Reservation,
}
impl PublicationClaim {
    fn charge(
        creator: crate::memory::cell_quota::Charge,
        retained: usize,
        transient: usize,
    ) -> Result<Self, ViError> {
        let total = retained
            .checked_add(transient)
            .ok_or(ViError::OutOfMemory)?;
        let mut transient = crate::memory::cell_quota::Reservation::take(
            creator,
            crate::memory::cell_quota::Account::Memory,
            total,
        )
        .map_err(|_| ViError::OutOfMemory)?;
        let retained = transient
            .split(retained)
            .expect("the claim covers its retained part");
        Ok(Self {
            retained,
            transient,
        })
    }
    /// Commit: the queue slot is consumed by the runnable task, so only that
    /// transient part is refunded; the retained log history stays attributed.
    fn transfer(self) {
        let Self {
            retained,
            transient,
        } = self;
        retained.keep();
        drop(transient);
    }
}

/// The ADR-0019 decision at a session commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DomainCommit {
    /// Not a domain child, or a domain child whose lease and grant state allow it.
    Admit,
    /// The admission lease no longer holds (a drain since prepare): `Revoked`.
    Drained,
    /// A domain-backed grant record is live: `Denied`.
    LiveGrant,
}

/// `lease_enabled` is `None` for an ordinary child. A drain wins over a live
/// grant: the policy generation the child was admitted under is gone, so the
/// grant state no longer matters.
pub(crate) fn domain_commit_gate(lease_enabled: Option<bool>, grant_live: bool) -> DomainCommit {
    match lease_enabled {
        None => DomainCommit::Admit,
        Some(false) => DomainCommit::Drained,
        Some(true) if grant_live => DomainCommit::LiveGrant,
        Some(true) => DomainCommit::Admit,
    }
}
#[cfg(feature = "native-domains")]
type DomainLease = crate::loader::domain_admission::DomainAdmissionLease;
/// Builds without a domain backend never hold a lease: every domain request is
/// refused with `NotSupported` before one could exist.
#[cfg(not(feature = "native-domains"))]
#[derive(Clone, Copy)]
enum DomainLease {}
#[cfg(not(feature = "native-domains"))]
impl DomainLease {
    fn remains_enabled(self) -> bool {
        match self {}
    }
}

/// Session-path wrappers around the ADR-0019 refusals, so `cell_session` needs
/// no domain-backend knowledge. Without a backend there is no domain grant and
/// a domain child never reaches commit.
pub(crate) fn domain_grant_records_live() -> bool {
    #[cfg(feature = "native-domains")]
    return crate::task::syscall::domain_grant_records_live();
    #[cfg(not(feature = "native-domains"))]
    false
}
pub(crate) fn refuse_while_domain_grant_live() -> Result<(), ViError> {
    #[cfg(feature = "native-domains")]
    return crate::loader::domain_admission::refuse_while_domain_grant_live();
    #[cfg(not(feature = "native-domains"))]
    Ok(())
}
pub(crate) fn audit_domain_live_grant_refusal() {
    #[cfg(feature = "native-domains")]
    let _ = crate::loader::domain_admission::live_grant_refusal();
}
pub(crate) fn audit_domain_drain_refusal() {
    #[cfg(feature = "native-domains")]
    let _ = crate::loader::domain_admission::drain_refusal();
}
/// ADR-0019 §2.1, the single admission policy applied at every publication
/// point (`publish_prepared` and `SessionPublication`): take the lease before
/// anything is created, build the private root, recheck the lease, bind. A
/// denial is final: no domain and no SAS fallback. The caller still rechecks
/// the returned lease immediately before the task becomes runnable.
#[cfg(all(feature = "native-domains", cellos_wide))]
fn admit_and_bind_domain(
    task: &mut super::Task,
    path: &str,
    granted: super::cap::CapSet,
) -> Result<DomainLease, ViError> {
    let lease = crate::loader::domain_admission::admit_for_launch(path, granted)?;
    let (Some(ks), Some(us), Some(seg)) = (&task.kernel_stack, &task.user_stack, &task.segment_mem)
    else {
        log::error!(
            "[domain] cannot create Tier 2 domain for '{}': missing stacks or segments",
            task.name
        );
        return Err(ViError::InvalidInput);
    };
    let domain = crate::memory::address_space::create_cell_domain(
        ks,
        us,
        seg,
        #[cfg(feature = "p01-trace")]
        crate::p01_resources::owner(task),
    )
    .map_err(|_| ViError::OutOfMemory)?;
    if !lease.remains_enabled() {
        return Err(crate::loader::domain_admission::drain_refusal());
    }
    task.bind_address_space(domain);
    Ok(lease)
}

#[cfg(not(all(feature = "native-domains", cellos_wide)))]
fn admit_and_bind_domain(
    task: &mut super::Task,
    _path: &str,
    _granted: super::cap::CapSet,
) -> Result<DomainLease, ViError> {
    log::error!(
        "[domain] Tier 2 Native Domain requested for '{}' but unsupported on this architecture/configuration",
        task.name
    );
    Err(ViError::NotSupported)
}

fn log_domain_admitted(name: &str) {
    #[cfg(target_arch = "riscv64")]
    let arch_tag = "SATP";
    #[cfg(target_arch = "aarch64")]
    let arch_tag = "TTBR0";
    #[cfg(target_arch = "x86_64")]
    let arch_tag = "CR3";
    #[cfg(not(cellos_wide))]
    let arch_tag = "none";
    log::warn!(
        "[domain] admitted cell '{}' to Tier 2 Paged Domain ({} isolation)",
        name,
        arch_tag
    );
}

/// The task, quota, CSpace, measurement-log and ready-queue slots remain
/// detached promises until the single session owner transaction commits. This
/// is not a second live-owner table: the Cell owner slot is `Reserved`, which
/// no resolve/watch path can observe as live.
pub(crate) struct SessionPublication {
    pub(crate) task: alloc::boxed::Box<super::Task>,
    pub(crate) owner: abi::cell_owner::CellOwner,
    state: TaskLaunchState,
    quota: crate::memory::cell_quota::QuotaReservation,
    authority: crate::cell::cspace::AuthorityReservation,
    measurement: crate::measurement_log::PublicationReservation,
    ready: crate::sched::hart_local::ready::PublicationReservation,
    claim: PublicationClaim,
    /// ADR-0019 lease of a domain child, rechecked in `current()` under the same
    /// SCHEDULER section as `publish`; `None` for an ordinary child.
    domain_lease: Option<DomainLease>,
    /// The launch edge route this child was admitted under (`Path` for a kernel
    /// read, `Elf` for caller-supplied bytes); `current()` re-authorizes it.
    route: crate::loader::launch_profile::LaunchRoute,
}
impl SessionPublication {
    /// Every fallible allocation, quota, owner-slot, log and ready-queue claim
    /// happens here; `publish` allocates nothing and cannot fail.
    pub(crate) fn reserve(
        sched: &mut crate::sched::scheduler::Scheduler,
        prepared: super::PreparedElfTask,
        state: TaskLaunchState,
        route: crate::loader::launch_profile::LaunchRoute,
    ) -> Result<Self, ViError> {
        let Some(caller) = state.caller.as_ref() else {
            return Err(ViError::PermissionDenied);
        };
        if !authority_is_current(sched, caller) {
            return Err(ViError::PermissionDenied);
        }
        let creator = sched
            .tasks
            .get(&caller.tid)
            .map(|t| crate::memory::cell_quota::Charge::of(t))
            .ok_or(ViError::PermissionDenied)?;
        // A session child is an ordinary governed path launch: never a boot
        // singleton, hotswap replacement, or service route publisher. A domain
        // child goes through the same ADR-0019 policy as `publish_prepared`.
        if prepared.requested_cell_id() != CellId(0)
            || state.platform.is_some()
            || state.replacement.is_some()
            || state.routes.block_io
            || state.routes.input
        {
            return Err(ViError::PermissionDenied);
        }
        #[cfg(feature = "test-hooks")]
        if state.routes.development_silo {
            return Err(ViError::PermissionDenied);
        }
        // Heap bookkeeping for the unpublished task is kernel memory, exactly as
        // in `publish_prepared`; per-principal accounting is the explicit claim.
        let _kernel_alloc = crate::sched::scheduler::KernelAllocationContext::enter();
        let tid = sched.next_task_id;
        let next = tid.checked_add(1).ok_or(ViError::OutOfMemory)?;
        let generation = super::tcb::next_cell_generation();
        let quota = crate::memory::cell_quota::QuotaReservation::reserve_next(
            generation,
            crate::memory::cell_quota::Limits {
                memory: state.quota_limit,
            },
        )?;
        let cell = quota.cell_id();
        let (mut task, _) = prepared.into_task(tid, cell, generation)?;
        task.root_tid = tid;
        task.syscall_allowlist = state.syscall_allowlist;
        task.priority = state.priority;
        task.pku_key = state.pku_key;
        task.pku_value = state.pku_value;
        task.is_critical = state.is_critical;
        state.granted.apply_to(&mut task);
        if let Some(measurement) = state.measurement.as_ref() {
            task.bind_image(measurement.image);
            task.launch_named = measurement.launch_named;
        }
        let domain_lease = if state.is_domain {
            let path = state.measurement.as_ref().map_or("", |m| m.path.as_str());
            Some(admit_and_bind_domain(&mut task, path, state.granted)?)
        } else {
            None
        };
        #[cfg(feature = "test-hooks")]
        {
            task.development_silo_registration_cap = state
                .routes
                .development_silo
                .then(super::cap::DevelopmentSiloRegistrationCap::new);
        }
        let owner = abi::cell_owner::CellOwner::new(cell.0, task.cell_generation, tid as u64);
        let authority = crate::cell::cspace::AuthorityReservation::reserve(cell, owner.generation)?;
        let path_len = state.measurement.as_ref().map_or(0, |m| m.path.len());
        let claim = PublicationClaim::charge(
            creator,
            crate::measurement_log::PublicationReservation::ENTRY_BYTES + path_len,
            crate::sched::hart_local::ready::PublicationReservation::SLOT_BYTES,
        )?;
        let measurement = crate::measurement_log::PublicationReservation::reserve((
            creator.cell().0,
            creator.generation(),
        ))?;
        let ready =
            crate::sched::hart_local::ready::PublicationReservation::reserve(state.priority)?;
        sched.reserve_session_owner(owner)?;
        sched.next_task_id = next;
        Ok(Self {
            task,
            owner,
            state,
            quota,
            authority,
            measurement,
            ready,
            claim,
            domain_lease,
            route,
        })
    }
    /// ADR-0019 commit decision for this child, given the live-grant preflight
    /// `dispatch` took before SCHEDULER. Call under SCHEDULER, right before
    /// `publish`, after `current()`.
    pub(crate) fn domain_commit(&self, grant_live: bool) -> DomainCommit {
        domain_commit_gate(
            self.domain_lease.map(|lease| lease.remains_enabled()),
            grant_live,
        )
    }
    pub(crate) fn current(&self, sched: &crate::sched::scheduler::Scheduler) -> bool {
        if !sched.session_owner_reserved(self.owner)
            || self
                .state
                .caller
                .as_ref()
                .is_none_or(|a| !authority_is_current(sched, a))
        {
            return false;
        }
        let Some(caller_tid) = self.state.caller.as_ref().map(|a| a.tid) else {
            return false;
        };
        // The caller's measured launch identity, never its task name (C-1).
        let Some(identity) = super::syscall::launch_identity_in(sched, caller_tid) else {
            return false;
        };
        let rights = sched.cell_rights(caller_tid);
        let Some(measurement) = self.state.measurement.as_ref() else {
            return false;
        };
        let state = crate::loader::launch_profile::CallerLaunchState {
            name: &identity,
            has_spawn: rights.spawn,
            has_supervisor: rights.supervisor,
        };
        crate::loader::launch_profile::authorize(state, self.route, &measurement.path).is_some_and(
            |profile| {
                self.state.granted.intersect(profile.child_ceiling) == self.state.granted
                    && (!profile.requires_lifecycle_authority || rights.spawn)
            },
        )
    }
    /// Final publication. Called with every owner check already passed, under
    /// SCHEDULER, and performs no allocation: capacity for the task row, CSpace
    /// row, log entry and ready entry was promised in `reserve`.
    pub(crate) fn publish(mut self, sched: &mut crate::sched::scheduler::Scheduler) {
        let tid = self.owner.root_tid as usize;
        if self.domain_lease.is_some() {
            log_domain_admitted(&self.task.name);
        }
        #[cfg(feature = "p01-trace")]
        {
            crate::memory::heap::p01_owner::adopt_task(&self.task);
            crate::memory::heap::p01_owner::task_backing(&self.task);
        }
        self.authority.commit();
        sched.tasks.publish_reserved(tid, self.task);
        sched.publish_session_owner(self.owner);
        self.quota.commit();
        crate::loader::commit_launch_routes(tid, self.state.routes);
        #[cfg(feature = "p01-trace")]
        {
            let task = sched.tasks.get_mut(&tid).expect("committed publication");
            let diagnostic_owner = crate::p01_measurement::Owner::of(task);
            if let Some(measurement) = self.state.measurement.as_ref() {
                if self.state.bootstrap_init {
                    crate::p01_measurement::arm(diagnostic_owner, measurement.digest);
                } else {
                    crate::p01_measurement::publication(
                        diagnostic_owner,
                        self.state.trace,
                        measurement.digest,
                    );
                }
            }
            crate::memory::heap::p01_owner::bind(self.state.trace, diagnostic_owner);
            crate::p01_resources::published(task);
        }
        if let Some(measurement) = self.state.measurement.take() {
            self.measurement
                .commit(tid, measurement.path, measurement.digest);
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

#[cfg(test)]
mod domain_commit_tests {
    use super::{domain_commit_gate, DomainCommit};

    #[test]
    fn ordinary_child_ignores_domain_state() {
        assert_eq!(domain_commit_gate(None, true), DomainCommit::Admit);
        assert_eq!(domain_commit_gate(None, false), DomainCommit::Admit);
    }

    #[test]
    fn domain_child_is_refused_by_drain_or_live_grant() {
        assert_eq!(domain_commit_gate(Some(true), false), DomainCommit::Admit);
        assert_eq!(
            domain_commit_gate(Some(true), true),
            DomainCommit::LiveGrant
        );
        assert_eq!(
            domain_commit_gate(Some(false), false),
            DomainCommit::Drained
        );
    }

    #[test]
    fn drain_takes_precedence_over_a_live_grant() {
        assert_eq!(domain_commit_gate(Some(false), true), DomainCommit::Drained);
    }
}
