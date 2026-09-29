//! Tier 2 admission policy — the single on-path control for domain launches.
//!
//! ADR-0019: a domain launch is admitted only while this policy is `ENABLED`, the
//! build has a qualified backend, the architecture is covered by that backend,
//! the artifact class is domain-eligible, the copied-IPC boundary exists, the
//! resource quota is available, and the requested authority is one a domain root
//! can actually enforce. The loader evaluates the policy before it creates
//! anything, holds the returned lease across every fallible admission step, and
//! re-checks it immediately before publication — so a concurrent drain either
//! linearizes before the admission or is observed by it, never in between.
//!
//! The cargo feature `native-domains` only selects whether a backend is compiled;
//! it is never an admission decision (ADR-0019 §2.2). A build with the backend
//! and no enabled policy denies domain-class artifacts; it never falls back to
//! the shared address space.

use crate::task::cap::CapSet;
use core::sync::atomic::{AtomicU64, Ordering};
use types::ViError;

const DISABLED: u64 = 0;
const ENABLED: u64 = 1;
const DRAINING: u64 = 2;
static POLICY: AtomicU64 = AtomicU64::new(DISABLED);
static GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainAdmissionDenial {
    FeatureDisabled,
    UnsupportedArchitecture,
    PolicyDisabled,
    PolicyDraining,
    ResourceQuota,
    ArtifactIneligible,
    CopiedIpcUnavailable,
    UnenforceableCapability,
    /// This architecture's raw context switch activates the incoming root before
    /// the outgoing context is saved (phase-02 gate).
    SwitchOrderingUnqualified,
    /// A grant record owned by, or shared to, a private-root task is live, so the
    /// containment gate cannot guarantee its receiver PTEs are revoked.
    LiveDomainGrant,
}

impl DomainAdmissionDenial {
    /// Stable code for the audit ring, so a denial is greppable after the fact.
    pub(crate) fn audit_code(self) -> u32 {
        match self {
            Self::FeatureDisabled => 1,
            Self::UnsupportedArchitecture => 2,
            Self::PolicyDisabled => 3,
            Self::PolicyDraining => 4,
            Self::ResourceQuota => 5,
            Self::ArtifactIneligible => 6,
            Self::CopiedIpcUnavailable => 7,
            Self::UnenforceableCapability => 8,
            Self::SwitchOrderingUnqualified => 9,
            Self::LiveDomainGrant => 10,
        }
    }

    /// The error the loader returns. A denial is final: it never downgrades to
    /// SAS, and it never publishes a partial task or domain.
    pub(crate) fn error(self) -> ViError {
        match self {
            Self::FeatureDisabled
            | Self::UnsupportedArchitecture
            | Self::SwitchOrderingUnqualified => ViError::NotSupported,
            Self::ResourceQuota => ViError::OutOfMemory,
            Self::PolicyDisabled
            | Self::PolicyDraining
            | Self::ArtifactIneligible
            | Self::CopiedIpcUnavailable
            | Self::UnenforceableCapability
            | Self::LiveDomainGrant => ViError::PermissionDenied,
        }
    }
}

/// A held policy generation. Publication must recheck it after every fallible
/// operation; a drain invalidates outstanding leases before it can commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DomainAdmissionLease(u64);

impl DomainAdmissionLease {
    pub(crate) fn remains_enabled(self) -> bool {
        POLICY.load(Ordering::Acquire) == ENABLED && GENERATION.load(Ordering::Acquire) == self.0
    }
}

/// Kernel-only request shape. It intentionally has no manifest/parser form:
/// the on-disk artifact states a class, never an admission decision.
#[derive(Clone, Copy)]
pub(crate) struct DomainAdmissionRequest {
    pub(crate) resource_quota_available: bool,
    pub(crate) artifact_eligible: bool,
    pub(crate) copied_ipc_ready: bool,
    pub(crate) requested_caps: CapSet,
    pub(crate) requests_dma: bool,
}

impl DomainAdmissionRequest {
    #[cfg(all(
        feature = "native-domains",
        feature = "test-hooks",
        any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        )
    ))]
    pub(crate) const fn fixture() -> Self {
        Self {
            resource_quota_available: true,
            artifact_eligible: true,
            copied_ipc_ready: true,
            requested_caps: CapSet::EMPTY,
            requests_dma: false,
        }
    }
}

/// Is this architecture's domain switch ordering qualified?
///
/// Phase-01 held every architecture but RV64 closed: `task.rs` activated the
/// incoming private root before `Context::switch` saved the outgoing context, so
/// a domain could go live with unsaved outgoing state. Phase 02 moved the root
/// write *inside* the switch — `Context::switch_with_root` programs `TTBR0_EL1`
/// after the outgoing context is stored and before the incoming stack is
/// adopted — and proved on one AArch64 CPU that the root-*writing* path executes
/// for SAS→domain and domain→domain, that the live `TTBR0_EL1` carries the
/// expected ASID at every stop, that the kernel root is restored afterwards, and
/// that a real domain cell runs at EL0 under its own root (`S22-AARCH64-ROOT-SWITCH`,
/// `S22-AARCH64-DOMAIN-LIVE`).
///
/// The ordering change is structural hardening, not a demonstrated fix: the A/B
/// in `a77545341` inverted the write order and the fixture still passed, because
/// the AArch64 save writes only to the context struct and reads no stack memory.
/// What the reopen rests on is therefore the executed root-writing path plus the
/// per-root isolation witnesses, not a discriminating ordering test.
///
/// x86_64 grew the same in-switch root write — `switch_with_root` composes CR3
/// with `domain::cr3_for` and programs it between the outgoing save and the
/// incoming stack adopt — and the phase-02 x86 test image now enters a real
/// domain cell through it, reading the live CR3 from inside the domain's own
/// kernel context (`S22-X86-DOMAIN-LIVE`) and tearing the root down with every
/// frame returned and nothing quarantined.
///
/// Every reopen is **test-images only**. A production AArch64 or x86_64 build
/// (no `test-hooks`) keeps refusing, which is the phase-01 posture the fleet
/// profile depends on, and the compile-time pin below makes that structural
/// rather than a matter of reading the cfg.
pub(crate) const fn switch_ordering_qualified() -> bool {
    cfg!(target_arch = "riscv64")
        || cfg!(all(target_arch = "aarch64", feature = "test-hooks"))
        || cfg!(all(target_arch = "x86_64", feature = "test-hooks"))
}

/// Phase-02 reopen is confined to test images.
///
/// `enable_for_boot` returns `false` (and the boot stays `PolicyDisabled`) on
/// every build this const-asserts about, so a production AArch64 or x86_64 image
/// cannot admit a domain-class cell by accident. Const-evaluated, so it fails the
/// build rather than a boot.
#[cfg(all(
    feature = "native-domains",
    any(target_arch = "aarch64", target_arch = "x86_64"),
    not(feature = "test-hooks")
))]
const _: () = assert!(!switch_ordering_qualified());

/// Does this build contain a domain backend for this architecture?
///
/// This is the same cfg set as the route that creates a domain in
/// `task::launch`; the policy and the route must not disagree about coverage
/// (ADR-0019 §2.4).
pub(crate) const fn architecture_covered() -> bool {
    cfg!(feature = "native-domains")
        && cfg!(any(
            target_arch = "riscv64",
            target_arch = "aarch64",
            target_arch = "x86_64"
        ))
}

/// Authority a Tier 2 domain root cannot enforce, and therefore must not hold.
///
/// A domain root maps only the cell's own image, stack, heap and explicit
/// grants: it has no user MMIO window, and `RequestMmio` has no domain-PTE route,
/// so device and DMA authority in a domain cell is dead authority whose request
/// path would still mutate the shared root. Device-class cells (drivers,
/// `/bin/vfs`, `/bin/platform`) are Tier 1 for that reason. Authority a domain
/// *can* enforce — network client capability, spawn authority bounded by this
/// same policy for every child, and service-registration authority — stays
/// admissible.
pub(crate) fn unenforceable_authority(caps: CapSet, requests_dma: bool) -> bool {
    requests_dma
        || caps.mmio_devices != 0
        || caps.pcie_driver
        || caps.usb_driver
        || caps.platform
        || caps.block_io
        || caps.block_regions != 0
        || caps.hypervisor
        || caps.supervisor
}

/// Build the request for a domain-class launch from the state the loader holds.
///
/// The only caller is `task::launch::publish_prepared`, the single publication
/// point for ELF tasks; a second admission path would defeat ADR-0019 §2.1.
pub(crate) fn evaluate_for_launch(
    granted: CapSet,
) -> Result<DomainAdmissionLease, DomainAdmissionDenial> {
    evaluate_domain_admission(DomainAdmissionRequest {
        // The quota reservation is taken before this call, so an unavailable
        // quota surfaces as `OutOfMemory` from the reservation itself.
        resource_quota_available: true,
        // `is_domain` is the class decision (unsigned, FFI, UNTRUSTED) that the
        // signature gate already made; authority is bounded below instead.
        artifact_eligible: true,
        copied_ipc_ready: architecture_covered(),
        requested_caps: granted,
        requests_dma: granted.pcie_driver || granted.platform || granted.usb_driver,
    })
}

/// Evaluate the launch and audit a denial before it becomes an error.
///
/// The audit record is what makes a refusal observable after the fact: a cell
/// that never appears because admission refused it leaves no task to inspect.
pub(crate) fn admit_for_launch(granted: CapSet) -> Result<DomainAdmissionLease, ViError> {
    evaluate_for_launch(granted).map_err(|denial| {
        crate::audit::log_event(
            crate::audit::AuditEvent::CellSpawnDenied,
            &crate::audit::encode_u32x2(0, denial.audit_code()),
        );
        denial.error()
    })
}

/// Refuse a domain-class publication while a domain-backed grant record is live.
///
/// Deliberately **not** part of [`evaluate_domain_admission`]: the caller
/// (`task::launch::publish_prepared`) holds the scheduler lock across that
/// evaluation, and the grant tables must only ever be taken in the documented
/// `*_GRANT_TABLE → SCHEDULER` order — never underneath `SCHEDULER`. The
/// publication path calls this first, before taking that lock, so the preflight
/// keeps its on-path position without the inversion.
///
/// A denial is final and audited: no task, no domain, and no SAS fallback.
pub(crate) fn refuse_while_domain_grant_live() -> Result<(), ViError> {
    if !crate::task::syscall::domain_grant_records_live() {
        return Ok(());
    }
    let denial = DomainAdmissionDenial::LiveDomainGrant;
    crate::audit::log_event(
        crate::audit::AuditEvent::CellSpawnDenied,
        &crate::audit::encode_u32x2(0, denial.audit_code()),
    );
    Err(denial.error())
}

/// Refuse a launch whose lease died between creation and publication.
pub(crate) fn drain_refusal() -> ViError {
    let denial = DomainAdmissionDenial::PolicyDraining;
    crate::audit::log_event(
        crate::audit::AuditEvent::CellSpawnDenied,
        &crate::audit::encode_u32x2(0, denial.audit_code()),
    );
    denial.error()
}

/// Evaluate all enforceable predicates before a builder or task can exist.
pub(crate) fn evaluate_domain_admission(
    request: DomainAdmissionRequest,
) -> Result<DomainAdmissionLease, DomainAdmissionDenial> {
    if !cfg!(feature = "native-domains") {
        return Err(DomainAdmissionDenial::FeatureDisabled);
    }
    if !architecture_covered() {
        return Err(DomainAdmissionDenial::UnsupportedArchitecture);
    }
    if !switch_ordering_qualified() {
        return Err(DomainAdmissionDenial::SwitchOrderingUnqualified);
    }
    match POLICY.load(Ordering::Acquire) {
        ENABLED => {}
        DRAINING => return Err(DomainAdmissionDenial::PolicyDraining),
        _ => return Err(DomainAdmissionDenial::PolicyDisabled),
    }
    if !request.resource_quota_available {
        return Err(DomainAdmissionDenial::ResourceQuota);
    }
    if !request.artifact_eligible {
        return Err(DomainAdmissionDenial::ArtifactIneligible);
    }
    if !request.copied_ipc_ready {
        return Err(DomainAdmissionDenial::CopiedIpcUnavailable);
    }
    if unenforceable_authority(request.requested_caps, request.requests_dma) {
        return Err(DomainAdmissionDenial::UnenforceableCapability);
    }
    Ok(DomainAdmissionLease(GENERATION.load(Ordering::Acquire)))
}

/// Enter the boot's admission posture. Called once during boot for a profile
/// that runs domain-class cells; a build that never calls it denies them
/// (fail-closed), which is the fleet posture.
pub(crate) fn enable_for_boot() -> bool {
    // Phase-01 posture: a build whose raw switch ordering is unqualified stays
    // fail-closed instead of entering the enabled posture, because the admission
    // control must never be stronger than the mechanism it guards. On AArch64
    // that is every production build; the test-hooks image qualifies and enables.
    if !switch_ordering_qualified() {
        return false;
    }
    // A PE that cannot program a private root cannot run one. At EL2 the AArch64
    // switch has no root argument (`__switch_el2`, and `switch_with_root` asserts
    // its absence), so a "qualified" ordering there would be a claim about a
    // mechanism the machine does not have. Refuse the posture rather than admit a
    // domain cell that would enter without its own mappings.
    #[cfg(target_arch = "aarch64")]
    if crate::hal::aarch64::el2::is_el2() {
        return false;
    }
    // The x86_64 analogue: a PE that never recorded its own kernel CR3 cannot
    // run a private root either. Admission runs after `init_kernel_paging_x86`
    // published the kernel PML4 in this boot, but the switch composes the
    // domain CR3 against a *known* kernel root, and trap entry only knows to
    // install it from `VI_KERNEL_CR3`. With that unset a "qualified" ordering
    // would be a claim about a mechanism this boot does not have — the same
    // refusal, for the same reason, as the EL2 branch above.
    #[cfg(target_arch = "x86_64")]
    if crate::hal::domain::kernel_cr3() == 0 {
        return false;
    }
    POLICY
        .compare_exchange(DISABLED, ENABLED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// Begin the boot-local one-way rollback. Existing domain teardown owns the
/// transition from draining to disabled; no caller may re-enable this boot.
#[allow(dead_code)] // reason: emergency admission rollback; no production caller until an operator channel lands (ADR-0019 §2.1), exercised by the boot selftest today
pub(crate) fn begin_domain_drain() -> bool {
    POLICY
        .compare_exchange(ENABLED, DRAINING, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(crate) fn policy_is_enabled() -> bool {
    POLICY.load(Ordering::Acquire) == ENABLED
}

/// Marker prefix for the boot selftest: the RV64 lane greps `S22-RV64-ADMISSION-*`,
/// the AArch64 lane `S22-AARCH64-ADMISSION-*`, the x86_64 lane `S22-X86-ADMISSION-*`.
/// The rendered marker text on RV64 is byte-identical to the pre-phase-02 literals,
/// so no existing assertion moved.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
const ADMISSION_TAG: &str = "RV64";
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "aarch64"
))]
const ADMISSION_TAG: &str = "AARCH64";
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "x86_64"
))]
const ADMISSION_TAG: &str = "X86";

/// Boot-time assertions for the admission control. The posture cases are the
/// evidence for ADR-0019: the policy denies when disabled or draining, an
/// outstanding lease dies with a drain, and the *publication path* refuses a
/// domain-class launch with nothing published and no SAS fallback.
///
/// The denial cases are the reason this runs on every architecture that can
/// carry a private root: `DENY`/`DRAIN`/`PUBLICATION-DENY` are the fleet
/// posture, asserted on the same code path the fleet profile leaves closed.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(crate) fn run_selftest() -> bool {
    let mut all_ok = true;

    // The boot posture is part of the contract: a domain-class cell only runs
    // because boot enabled admission, not because a default feature admitted it.
    let enabled = policy_is_enabled();
    all_ok &= enabled;
    if enabled {
        log::info!("S22-{}-ADMISSION-ENABLED: PASS", ADMISSION_TAG);
    } else {
        log::error!(
            "S22-{}-ADMISSION-ENABLED: FAIL — boot posture did not enable admission",
            ADMISSION_TAG
        );
    }

    // Disabled posture denies, and a denial maps to a final error, never SAS.
    POLICY.store(DISABLED, Ordering::Release);
    let disabled = evaluate_domain_admission(DomainAdmissionRequest::fixture())
        == Err(DomainAdmissionDenial::PolicyDisabled)
        && DomainAdmissionDenial::PolicyDisabled.error() == ViError::PermissionDenied;
    all_ok &= disabled;
    if disabled {
        log::info!("S22-{}-ADMISSION-DENY: PASS", ADMISSION_TAG);
    } else {
        log::error!("S22-{}-ADMISSION-DENY: FAIL", ADMISSION_TAG);
    }

    // Enabled admits, and a drain invalidates the lease an admission is holding.
    POLICY.store(ENABLED, Ordering::Release);
    let lease = evaluate_domain_admission(DomainAdmissionRequest::fixture());
    let drained = begin_domain_drain()
        && lease.is_ok_and(|lease| !lease.remains_enabled())
        && evaluate_domain_admission(DomainAdmissionRequest::fixture())
            == Err(DomainAdmissionDenial::PolicyDraining);
    all_ok &= drained;
    if drained {
        log::info!("S22-{}-ADMISSION-DRAIN: PASS", ADMISSION_TAG);
    } else {
        log::error!("S22-{}-ADMISSION-DRAIN: FAIL", ADMISSION_TAG);
    }

    // The route itself: while draining, the single publication point must refuse
    // a domain-class launch with no task, no domain, and no SAS fallback.
    let publication_denied = publication_is_refused_while_draining();
    all_ok &= publication_denied;
    if publication_denied {
        log::info!("S22-{}-ADMISSION-PUBLICATION-DENY: PASS", ADMISSION_TAG);
    } else {
        log::error!("S22-{}-ADMISSION-PUBLICATION-DENY: FAIL", ADMISSION_TAG);
    }

    // Unenforceable authority is refused by name, so a device-class artifact
    // cannot be admitted as a domain by a future manifest edit.
    let mut device_caps = CapSet::EMPTY;
    device_caps.mmio_devices = 0b1;
    let ceiling = unenforceable_authority(device_caps, false)
        && unenforceable_authority(CapSet::EMPTY, true)
        && !unenforceable_authority(CapSet::EMPTY, false);
    all_ok &= ceiling;
    if ceiling {
        log::info!("S22-{}-ADMISSION-CEILING: PASS", ADMISSION_TAG);
    } else {
        log::error!("S22-{}-ADMISSION-CEILING: FAIL", ADMISSION_TAG);
    }

    // Leave the boot in the posture the boot policy chose: the cases above moved
    // the policy for their own observation, and the rest of this boot runs cells.
    POLICY.store(ENABLED, Ordering::Release);
    all_ok
}

/// Drive the real publication path with a domain-class launch while the policy
/// is draining and observe that nothing is published.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
fn publication_is_refused_while_draining() -> bool {
    use crate::task::{LaunchRoutes, TaskLaunchState};

    // AArch64 and x86_64 have no `domain_identity_counter`, so the "no domain was
    // created" half of this proof is carried by the ledger a private root is
    // made of: the
    // snapshot is taken *before* the ELF is prepared, so a refusal that publishes
    // nothing must return the ledger to exactly this value — the task's own
    // stacks and segments are allocated and released inside the call, and a
    // private root and its tables are frames too. (Taken after preparation the
    // comparison is one-sided and meaningless: the refusal's release of the
    // task's own frames reads as a decrease.)
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    let frames_before = frames_in_use();
    let Ok(prepared) = crate::task::prepare_elf_task(
        crate::INIT_ELF,
        "admission-publication-probe",
        types::CellId(0),
        alloc::vec::Vec::new(),
    ) else {
        return false;
    };
    let state = TaskLaunchState::complete(
        None,
        CapSet::EMPTY,
        None,
        None,
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
        u64::MAX,
        0,
        0,
        api::TaskPriority::Normal as u8,
        0,
        0,
        false,
        0,
        None,
        LaunchRoutes {
            block_io: false,
            input: false,
            development_silo: false,
        },
        None,
        true,
    );

    let tasks_before = crate::task::scheduler_stats().0;
    #[cfg(target_arch = "riscv64")]
    let domains_before = crate::memory::address_space::domain_identity_counter();
    let outcome = crate::task::publish_prepared(prepared, state);
    let tasks_after = crate::task::scheduler_stats().0;
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    let frames_after = frames_in_use();
    // `domain_identity_counter` is RV64-only, but the claim it carries — a refused
    // launch creates no domain — is arch-neutral and is the one that matters: a
    // private root and its tables *are* frames, so a refusal that still moved the
    // frame ledger would mean a domain was built and retained for a launch that
    // never published. `create_cell_domain` is the only allocator on this path,
    // and the drain refusal precedes it.
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    let domains_unchanged = frames_before.is_some() && frames_after == frames_before;
    #[cfg(target_arch = "riscv64")]
    let domains_unchanged =
        crate::memory::address_space::domain_identity_counter() == domains_before;

    let refused = outcome == Err(ViError::PermissionDenied);
    let unpublished = tasks_after == tasks_before;
    if !(refused && unpublished && domains_unchanged) {
        log::error!(
            "S22-{}-ADMISSION-PUBLICATION-DENY detail: refused={} unpublished={} no_domain={} tasks {}->{} outcome={:?}",
            ADMISSION_TAG,
            refused,
            unpublished,
            domains_unchanged,
            tasks_before,
            tasks_after,
            outcome
        );
        #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
        log::error!(
            "S22-{}-ADMISSION-PUBLICATION-DENY detail: frames {:?} -> {:?}",
            ADMISSION_TAG,
            frames_before,
            frames_after
        );
    }

    refused && unpublished && domains_unchanged
}

/// Frames the frame allocator has handed out, or `None` before it is published.
///
/// AArch64 and x86_64 have no domain-identity counter, so the refusal proof above
/// is carried by the ledger a private root is actually made of. Its only caller is
/// the test-hooks publication probe, so it carries that caller's cfg — a
/// production AArch64 or x86_64 build has no use for it and `-D warnings` refuses
/// dead code.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
fn frames_in_use() -> Option<usize> {
    crate::memory::frame::FRAME_ALLOCATOR
        .lock()
        .as_ref()
        .map(|allocator| allocator.used_frames())
}
