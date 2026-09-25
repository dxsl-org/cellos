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
        }
    }

    /// The error the loader returns. A denial is final: it never downgrades to
    /// SAS, and it never publishes a partial task or domain.
    pub(crate) fn error(self) -> ViError {
        match self {
            Self::FeatureDisabled | Self::UnsupportedArchitecture => ViError::NotSupported,
            Self::ResourceQuota => ViError::OutOfMemory,
            Self::PolicyDisabled
            | Self::PolicyDraining
            | Self::ArtifactIneligible
            | Self::CopiedIpcUnavailable
            | Self::UnenforceableCapability => ViError::PermissionDenied,
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
    #[cfg(feature = "test-hooks")]
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

#[cfg(feature = "test-hooks")]
pub(crate) fn policy_is_enabled() -> bool {
    POLICY.load(Ordering::Acquire) == ENABLED
}

/// Boot-time assertions for the admission control. The posture cases are the
/// evidence for ADR-0019: the policy denies when disabled or draining, an
/// outstanding lease dies with a drain, and the *publication path* refuses a
/// domain-class launch with nothing published and no SAS fallback.
#[cfg(feature = "test-hooks")]
pub(crate) fn run_selftest() {
    // The boot posture is part of the contract: a domain-class cell only runs
    // because boot enabled admission, not because a default feature admitted it.
    if policy_is_enabled() {
        log::info!("S22-RV64-ADMISSION-ENABLED: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-ENABLED: FAIL — boot posture did not enable admission");
    }

    // Disabled posture denies, and a denial maps to a final error, never SAS.
    POLICY.store(DISABLED, Ordering::Release);
    let disabled = evaluate_domain_admission(DomainAdmissionRequest::fixture())
        == Err(DomainAdmissionDenial::PolicyDisabled)
        && DomainAdmissionDenial::PolicyDisabled.error() == ViError::PermissionDenied;
    if disabled {
        log::info!("S22-RV64-ADMISSION-DENY: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-DENY: FAIL");
    }

    // Enabled admits, and a drain invalidates the lease an admission is holding.
    POLICY.store(ENABLED, Ordering::Release);
    let lease = evaluate_domain_admission(DomainAdmissionRequest::fixture());
    let drained = begin_domain_drain()
        && lease.is_ok_and(|lease| !lease.remains_enabled())
        && evaluate_domain_admission(DomainAdmissionRequest::fixture())
            == Err(DomainAdmissionDenial::PolicyDraining);
    if drained {
        log::info!("S22-RV64-ADMISSION-DRAIN: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-DRAIN: FAIL");
    }

    // The route itself: while draining, the single publication point must refuse
    // a domain-class launch with no task, no domain, and no SAS fallback.
    if publication_is_refused_while_draining() {
        log::info!("S22-RV64-ADMISSION-PUBLICATION-DENY: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-PUBLICATION-DENY: FAIL");
    }

    // Unenforceable authority is refused by name, so a device-class artifact
    // cannot be admitted as a domain by a future manifest edit.
    let mut device_caps = CapSet::EMPTY;
    device_caps.mmio_devices = 0b1;
    let ceiling = unenforceable_authority(device_caps, false)
        && unenforceable_authority(CapSet::EMPTY, true)
        && !unenforceable_authority(CapSet::EMPTY, false);
    if ceiling {
        log::info!("S22-RV64-ADMISSION-CEILING: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-CEILING: FAIL");
    }

    // Leave the boot in the posture the boot policy chose: the cases above moved
    // the policy for their own observation, and the rest of this boot runs cells.
    POLICY.store(ENABLED, Ordering::Release);
}

/// Drive the real publication path with a domain-class launch while the policy
/// is draining and observe that nothing is published.
#[cfg(feature = "test-hooks")]
fn publication_is_refused_while_draining() -> bool {
    use crate::task::{LaunchRoutes, TaskLaunchState};

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
    let domains_before = crate::memory::address_space::domain_identity_counter();
    let outcome = crate::task::publish_prepared(prepared, state);
    let tasks_after = crate::task::scheduler_stats().0;
    let domains_after = crate::memory::address_space::domain_identity_counter();

    outcome == Err(ViError::PermissionDenied)
        && tasks_after == tasks_before
        && domains_after == domains_before
}
