//! Default-off, internal-only gate for future native-domain admission.
//!
//! A successful result is an immutable policy-generation lease, not permission
//! to expose a Tier-2 route. No public loader or manifest path constructs a
//! request; test hooks use this module solely to prove denial and drain rules.

#![allow(dead_code)]
use crate::task::cap::CapSet;
use core::sync::atomic::{AtomicU64, Ordering};

const DISABLED: u64 = 0;
const ENABLED: u64 = 1;
const DRAINING: u64 = 2;
static POLICY: AtomicU64 = AtomicU64::new(DISABLED);
static GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainAdmissionDenial {
    FeatureDisabled,
    PolicyDisabled,
    PolicyDraining,
    UnsupportedArchitecture,
    ResourceQuota,
    ArtifactIneligible,
    CopiedIpcUnavailable,
    UnenforceableCapability,
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

/// Kernel-only request shape. It intentionally has no manifest/parser form.
#[derive(Clone, Copy)]
pub(crate) struct DomainAdmissionRequest {
    pub(crate) resource_quota_available: bool,
    pub(crate) artifact_eligible: bool,
    pub(crate) copied_ipc_ready: bool,
    pub(crate) requested_caps: CapSet,
    pub(crate) requests_dma: bool,
}

impl DomainAdmissionRequest {
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

/// Evaluate all enforceable predicates before a builder or task can exist.
pub(crate) fn evaluate_domain_admission(
    request: DomainAdmissionRequest,
) -> Result<DomainAdmissionLease, DomainAdmissionDenial> {
    if !cfg!(feature = "native-domains") {
        return Err(DomainAdmissionDenial::FeatureDisabled);
    }
    if !cfg!(target_arch = "riscv64") {
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
    if request.requested_caps != CapSet::EMPTY || request.requests_dma {
        return Err(DomainAdmissionDenial::UnenforceableCapability);
    }
    Ok(DomainAdmissionLease(GENERATION.load(Ordering::Acquire)))
}

/// Begin the boot-local one-way rollback. Existing domain teardown owns the
/// transition from draining to disabled; no caller may re-enable this boot.
pub(crate) fn begin_domain_drain() -> bool {
    POLICY
        .compare_exchange(ENABLED, DRAINING, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

#[cfg(feature = "test-hooks")]
pub(crate) fn run_selftest() {
    let disabled = evaluate_domain_admission(DomainAdmissionRequest::fixture())
        == Err(DomainAdmissionDenial::PolicyDisabled);
    if disabled {
        log::info!("S22-RV64-ADMISSION-DENY: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-DENY: FAIL");
    }

    POLICY.store(ENABLED, Ordering::Release);
    let lease = evaluate_domain_admission(DomainAdmissionRequest::fixture());
    let drained = begin_domain_drain()
        && lease.is_ok_and(|lease| !lease.remains_enabled())
        && evaluate_domain_admission(DomainAdmissionRequest::fixture())
            == Err(DomainAdmissionDenial::PolicyDraining);
    POLICY.store(DISABLED, Ordering::Release);
    if drained {
        log::info!("S22-RV64-ADMISSION-DRAIN: PASS");
    } else {
        log::error!("S22-RV64-ADMISSION-DRAIN: FAIL");
    }
}
