use super::*;
use types::kms::{
    RelayCsrChunkRequestPayload, RelayCsrChunkResponsePayload, RelayEnrollmentAbortRequestPayload,
    RelayEnrollmentBeginRequestPayload, RelayEnrollmentBeginResponsePayload,
    RelayGenerationCommitRequestPayload, RelayStageProfileRequestPayload,
};

use super::tls_signing::sign_request;
use crate::storage::{FixtureRelayProvider, FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION};

use std::vec::Vec;

const HOSTNAME: &str = "relay.example.internal";
const SUPERVISOR: usize = 8;

fn begin_request() -> [u8; 128] {
    let mut payload = RelayEnrollmentBeginRequestPayload {
        hostname_len: HOSTNAME.len() as u8,
        hostname: [0; 64],
    };
    payload.hostname[..HOSTNAME.len()].copy_from_slice(HOSTNAME.as_bytes());
    request(KmsOpcode::BeginRelayEnrollment, &payload.encode())
}

fn supervisor_caller(tid: usize) -> Option<api::caller_identity::CallerIdentity> {
    Some(caller(90, 5, tid))
}

fn supervisor_registry() -> ServiceRegistrySnapshot {
    ServiceRegistrySnapshot {
        net_broker_tid: None,
        supervisor_tid: Some(SUPERVISOR),
        net_tid: None,
    }
}

/// Begin + read every chunk in order; returns the assembled CSR.
fn enroll(service: &mut KmsService) -> (RelayEnrollmentBeginResponsePayload, Vec<u8>) {
    let response = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_eq!(response.error_code().unwrap(), None);
    let begin = RelayEnrollmentBeginResponsePayload::decode(response.payload().unwrap()).unwrap();
    let mut csr = Vec::new();
    let total = begin.csr_len as usize;
    for index in 0..total.div_ceil(104) {
        let payload = RelayCsrChunkRequestPayload {
            csr_handle: begin.csr_handle,
            chunk_index: index as u32,
            reserved: 0,
        };
        let response = service
            .handle(
                &request(KmsOpcode::ReadRelayCsrChunk, &payload.encode()),
                SUPERVISOR,
                supervisor_caller(SUPERVISOR),
                supervisor_registry(),
            )
            .unwrap();
        let chunk = RelayCsrChunkResponsePayload::decode(response.payload().unwrap()).unwrap();
        csr.extend_from_slice(&chunk.chunk[..chunk.chunk_len as usize]);
    }
    assert_eq!(csr.len(), total);
    (begin, csr)
}

fn net_caller(tid: usize) -> Option<api::caller_identity::CallerIdentity> {
    Some(caller(60, 3, tid))
}

fn net_registry() -> ServiceRegistrySnapshot {
    ServiceRegistrySnapshot {
        net_broker_tid: None,
        supervisor_tid: Some(SUPERVISOR),
        net_tid: Some(7),
    }
}

fn bind_net(service: &mut KmsService, tid: usize, generation: u64) {
    let response = service
        .handle(
            &request(KmsOpcode::RegisterServiceNetInstance, &[]),
            tid,
            Some(caller(60, generation, tid)),
            ServiceRegistrySnapshot {
                net_broker_tid: None,
                supervisor_tid: Some(SUPERVISOR),
                net_tid: Some(tid),
            },
        )
        .unwrap();
    assert_eq!(response.error_code().unwrap(), None);
}

#[test]
fn runtime_callers_cannot_enroll_before_provider() {
    // Service-net, broker, and unattested callers are denied by
    // authorization before any provider key creation runs.
    for (tid, caller, registry) in [
        (7, net_caller(7), net_registry()),
        (
            6,
            Some(caller(20, 30, 6)),
            registry(Some(6), Some(SUPERVISOR)),
        ),
        (SUPERVISOR, None, supervisor_registry()),
    ] {
        let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
        let response = service
            .handle(&begin_request(), tid, caller, registry)
            .unwrap();
        assert!(
            response.error_code().unwrap().is_some(),
            "expected denial for tid {tid}"
        );
    }
}

#[test]
fn post_create_proof_failure_requires_confirmed_provider_cleanup() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static DESTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
    DESTRUCTIONS.store(0, Ordering::Relaxed);
    let provider = FixtureRelayProvider {
        behavior: crate::storage::FixtureSignatureBehavior::Corrupt,
        key_destructions: Some(&DESTRUCTIONS),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    let failed = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(failed, KmsErrorCode::InvalidSignature);
    assert_eq!(DESTRUCTIONS.load(Ordering::Relaxed), 1);
    assert!(service.lifecycle.pending().is_none());
}

#[test]
fn begin_publishes_bounded_canonical_csr_chunks_in_order() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    let (begin, csr) = enroll(&mut service);
    // The fixture lane already serves the pinned development generation,
    // so the first enrollment allocates the strictly next generation.
    assert_eq!(begin.pending_relay_generation, FIXTURE_RELAY_GENERATION + 1);
    assert_eq!(begin.restart_epoch, service.lifecycle.restart_epoch());
    assert!(begin.csr_len as usize <= types::kms::RELAY_CSR_MAX_LEN);
    assert_ne!(begin.csr_sha256, [0; 32]);
    // The CRI is embedded verbatim inside the assembled CSR; the outer
    // SEQUENCE uses the canonical minimal long-form length (0x81, one byte).
    assert_eq!(&csr[..2], &[0x30, 0x81]);
    let payload = RelayCsrChunkRequestPayload {
        csr_handle: begin.csr_handle,
        chunk_index: begin.csr_len / 104,
        reserved: 0,
    };
    let response = service
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &payload.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::CsrHandleInvalid);
}

#[test]
fn out_of_order_chunk_rejects_and_invalidates() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    let response = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    let begin = RelayEnrollmentBeginResponsePayload::decode(response.payload().unwrap()).unwrap();
    let skip = RelayCsrChunkRequestPayload {
        csr_handle: begin.csr_handle,
        chunk_index: 1,
        reserved: 0,
    };
    let bad = service
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &skip.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(bad, KmsErrorCode::CsrOrderInvalid);
    let replay = RelayCsrChunkRequestPayload {
        csr_handle: begin.csr_handle,
        chunk_index: 0,
        reserved: 0,
    };
    let gone = service
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &replay.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(gone, KmsErrorCode::CsrHandleInvalid);
}

#[test]
fn csr_handle_transfer_is_denied_and_invalidated() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    let response = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    let begin = RelayEnrollmentBeginResponsePayload::decode(response.payload().unwrap()).unwrap();
    let payload = RelayCsrChunkRequestPayload {
        csr_handle: begin.csr_handle,
        chunk_index: 0,
        reserved: 0,
    };
    let foreign = service
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &payload.encode()),
            SUPERVISOR,
            Some(caller(91, 6, SUPERVISOR)),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(foreign, KmsErrorCode::PermissionDenied);
    let owner = service
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &payload.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(owner, KmsErrorCode::PermissionDenied);
}

#[test]
fn commit_requires_staging_and_exact_digest_then_serves_new_tuple() {
    use types::kms::RelayGenerationCommitResponsePayload;
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    bind_net(&mut service, 7, 3);
    let (begin, _) = enroll(&mut service);
    let early = RelayGenerationCommitRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
        expected_policy_epoch: begin.policy_epoch,
        profile_digest: [0x55; 32],
    };
    let denied = service
        .handle(
            &request(KmsOpcode::CommitRelayGeneration, &early.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(denied, KmsErrorCode::InvalidRequest);

    let stage = RelayStageProfileRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
        expected_policy_epoch: begin.policy_epoch,
        profile_digest: [0x55; 32],
    };
    let staged = service
        .handle(
            &request(KmsOpcode::StageRelayProfile, &stage.encode()),
            7,
            net_caller(7),
            net_registry(),
        )
        .unwrap();
    assert_eq!(staged.error_code().unwrap(), None);

    let wrong = RelayGenerationCommitRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
        expected_policy_epoch: begin.policy_epoch,
        profile_digest: [0x56; 32],
    };
    let mismatch = service
        .handle(
            &request(KmsOpcode::CommitRelayGeneration, &wrong.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(mismatch, KmsErrorCode::InvalidRequest);

    let commit = RelayGenerationCommitRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
        expected_policy_epoch: begin.policy_epoch,
        profile_digest: [0x55; 32],
    };
    let committed = service
        .handle(
            &request(KmsOpcode::CommitRelayGeneration, &commit.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_eq!(committed.error_code().unwrap(), None);
    let committed =
        RelayGenerationCommitResponsePayload::decode(committed.payload().unwrap()).unwrap();
    assert_eq!(
        committed.active_relay_generation,
        begin.pending_relay_generation
    );

    // Status now serves the promoted tuple.
    bind_net(&mut service, 7, 3);
    let status = service
        .handle(
            &request(KmsOpcode::GetRelayP256Status, &[]),
            7,
            net_caller(7),
            net_registry(),
        )
        .unwrap();
    let metadata = types::kms::RelayP256StatusPayload::decode(status.payload().unwrap()).unwrap();
    assert_eq!(metadata.relay_generation, begin.pending_relay_generation);
    assert_eq!(metadata.active_profile_digest, [0x55; 32]);

    // Stale-generation TLS requests are rejected against the dynamic tuple.
    let stale = service
        .handle(
            &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99),
            7,
            net_caller(7),
            net_registry(),
        )
        .unwrap();
    assert_error(stale, KmsErrorCode::RelayUnavailable);
}

#[test]
fn active_public_key_reads_are_service_net_only() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    let denied = service
        .handle(
            &request(KmsOpcode::GetRelayActivePublicKey, &[]),
            9,
            Some(caller(20, 30, 9)),
            registry(Some(9), Some(SUPERVISOR)),
        )
        .unwrap();
    assert_error(denied, KmsErrorCode::ServiceBindingRequired);
    bind_net(&mut service, 7, 3);
    let ok = service
        .handle(
            &request(KmsOpcode::GetRelayActivePublicKey, &[]),
            7,
            net_caller(7),
            net_registry(),
        )
        .unwrap();
    assert_eq!(ok.error_code().unwrap(), None);
    let payload = types::kms::RelayActivePublicKeyPayload::decode(ok.payload().unwrap()).unwrap();
    assert_eq!(payload.spki_sec1[0], 4);
    assert_ne!(payload.spki_sha256, [0; 32]);
}

#[test]
fn restart_seals_prior_handles_and_commit_requires_fresh_enrollment() {
    // A fresh service is a simulated restart with a new monotonic epoch.
    let mut first = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    let (begin, _) = enroll(&mut first);
    let second = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    assert_ne!(
        second.lifecycle.restart_epoch(),
        first.lifecycle.restart_epoch()
    );
    // The pre-restart handle cannot read from the new process.
    let mut second = second;
    let payload = RelayCsrChunkRequestPayload {
        csr_handle: begin.csr_handle,
        chunk_index: 0,
        reserved: 0,
    };
    let replayed = second
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &payload.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(replayed, KmsErrorCode::CsrHandleInvalid);
}

#[test]
fn invalidation_cleanup_failure_is_retried_before_replacement() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static CREATIONS: AtomicUsize = AtomicUsize::new(0);
    static DESTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
    static FAILURES: AtomicUsize = AtomicUsize::new(1);
    CREATIONS.store(0, Ordering::Relaxed);
    DESTRUCTIONS.store(0, Ordering::Relaxed);
    FAILURES.store(1, Ordering::Relaxed);
    let provider = FixtureRelayProvider {
        key_creations: Some(&CREATIONS),
        key_destructions: Some(&DESTRUCTIONS),
        destroy_failures: Some(&FAILURES),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    let first = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    let first = RelayEnrollmentBeginResponsePayload::decode(first.payload().unwrap()).unwrap();
    let out_of_order = RelayCsrChunkRequestPayload {
        csr_handle: first.csr_handle,
        chunk_index: 1,
        reserved: 0,
    };
    let cleanup_failed = service
        .handle(
            &request(KmsOpcode::ReadRelayCsrChunk, &out_of_order.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(cleanup_failed, KmsErrorCode::RelayUnavailable);
    assert!(service.lifecycle.cleanup_pending().is_some());
    assert_eq!(DESTRUCTIONS.load(Ordering::Relaxed), 0);

    // Fresh Begin first reconciles the tombstone, then creates a replacement
    // for the same generation with a nonrepeating handle.
    let replacement = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    let replacement =
        RelayEnrollmentBeginResponsePayload::decode(replacement.payload().unwrap()).unwrap();
    assert_eq!(
        replacement.pending_relay_generation,
        first.pending_relay_generation
    );
    assert_ne!(replacement.csr_handle, first.csr_handle);
    assert_eq!(CREATIONS.load(Ordering::Relaxed), 2);
    assert_eq!(DESTRUCTIONS.load(Ordering::Relaxed), 1);
}

#[test]
fn abort_ack_waits_for_confirmed_destroy_and_remains_retryable() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static DESTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
    static FAILURES: AtomicUsize = AtomicUsize::new(1);
    DESTRUCTIONS.store(0, Ordering::Relaxed);
    FAILURES.store(1, Ordering::Relaxed);
    let provider = FixtureRelayProvider {
        key_destructions: Some(&DESTRUCTIONS),
        destroy_failures: Some(&FAILURES),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    let response = service
        .handle(
            &begin_request(),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    let begin = RelayEnrollmentBeginResponsePayload::decode(response.payload().unwrap()).unwrap();
    let abort = RelayEnrollmentAbortRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
    };
    let failed = service
        .handle(
            &request(KmsOpcode::AbortRelayEnrollment, &abort.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_error(failed, KmsErrorCode::RelayUnavailable);
    assert!(service.lifecycle.pending().is_some());

    let confirmed = service
        .handle(
            &request(KmsOpcode::AbortRelayEnrollment, &abort.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_eq!(confirmed.error_code().unwrap(), None);
    assert!(service.lifecycle.pending().is_none());
    assert_eq!(DESTRUCTIONS.load(Ordering::Relaxed), 1);
}

#[test]
fn committed_protected_state_recovers_generation_profile_and_floors() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    bind_net(&mut service, 7, 3);
    let (begin, _) = enroll(&mut service);
    let profile_digest = [0x66; 32];
    let stage = RelayStageProfileRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
        expected_policy_epoch: begin.policy_epoch,
        profile_digest,
    };
    let staged = service
        .handle(
            &request(KmsOpcode::StageRelayProfile, &stage.encode()),
            7,
            net_caller(7),
            net_registry(),
        )
        .unwrap();
    assert_eq!(staged.error_code().unwrap(), None);
    let commit = RelayGenerationCommitRequestPayload {
        pending_relay_generation: begin.pending_relay_generation,
        expected_policy_epoch: begin.policy_epoch,
        profile_digest,
    };
    let committed = service
        .handle(
            &request(KmsOpcode::CommitRelayGeneration, &commit.encode()),
            SUPERVISOR,
            supervisor_caller(SUPERVISOR),
            supervisor_registry(),
        )
        .unwrap();
    assert_eq!(committed.error_code().unwrap(), None);
    let protected = service.protected_lifecycle_for_tests().unwrap();
    let restart_epoch = protected.restart_epoch_floor + 1;
    let recovered = KmsService::with_recovered_provider_fixture(
        FixtureRelayProvider::production(),
        protected,
        restart_epoch,
    )
    .unwrap();
    let active = recovered.lifecycle.serving().unwrap();
    assert_eq!(active.generation, begin.pending_relay_generation);
    assert_eq!(active.policy_epoch, begin.policy_epoch);
    assert_eq!(active.profile_digest, profile_digest);
    assert!(matches!(
        KmsService::with_recovered_provider_fixture(
            FixtureRelayProvider::production(),
            protected,
            protected.restart_epoch_floor,
        ),
        Err(KmsErrorCode::PolicyEpochRegressed)
    ));
}
