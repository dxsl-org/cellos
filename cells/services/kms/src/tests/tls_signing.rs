use core::sync::atomic::{AtomicUsize, Ordering};

use types::kms::{
    KmsCapabilityReadiness, KmsErrorCode, KmsOpcode, RelayP256StatusPayload,
    RelayProviderAssessment, ServiceNetBindingPayload, Tls13ClientCertificateVerifyRequestPayload,
    Tls13ClientCertificateVerifyResponsePayload,
};

use crate::storage::{
    FixtureRelayProvider, FixtureSignatureBehavior, FIXTURE_PROFILE_DIGEST,
    FIXTURE_RELAY_GENERATION,
};

use super::*;

mod provider_output;
mod replay;

fn bind_net(service: &mut KmsService, tid: usize, generation: u64) -> ServiceNetBindingPayload {
    let response = service
        .handle(
            &request(KmsOpcode::RegisterServiceNetInstance, &[]),
            tid,
            Some(caller(50, generation, tid)),
            net_registry(Some(tid)),
        )
        .unwrap();
    ServiceNetBindingPayload::decode(response.payload().unwrap()).unwrap()
}

pub(super) fn sign_request(profile: [u8; 32], generation: u64, request_id: u64) -> [u8; 128] {
    request(
        KmsOpcode::SignTls13ClientCertificateVerify,
        &Tls13ClientCertificateVerifyRequestPayload {
            transcript_hash: [0x19; 32],
            relay_generation: generation,
            active_profile_digest: profile,
            request_id,
        }
        .encode(),
    )
}

#[test]
fn authorization_precedes_every_provider_access() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let provider = FixtureRelayProvider {
        access_calls: Some(&CALLS),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    let response = service
        .handle(
            &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(7)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::ServiceBindingRequired);
    assert_eq!(CALLS.load(Ordering::Relaxed), 0);
}

#[test]
fn stale_or_restarted_service_net_generation_is_denied() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    bind_net(&mut service, 7, 4);
    let frame = sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99);
    let stale_tid = service
        .handle(&frame, 7, Some(caller(50, 4, 7)), net_registry(Some(8)))
        .unwrap();
    assert_error(stale_tid, KmsErrorCode::ServiceBindingStale);
    let restarted = service
        .handle(&frame, 8, Some(caller(50, 5, 8)), net_registry(Some(8)))
        .unwrap();
    assert_error(restarted, KmsErrorCode::ServiceBindingStale);
}

#[test]
fn relay_readiness_is_independent_from_c2c_readiness() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let provider = FixtureRelayProvider {
        access_calls: Some(&CALLS),
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    assert_eq!(CALLS.load(Ordering::Relaxed), 0);
    bind_net(&mut service, 7, 4);
    let c2c = service
        .handle(
            &request(KmsOpcode::GetNodeIdentityStatus, &[]),
            8,
            Some(caller(1, 1, 8)),
            registry(None, Some(8)),
        )
        .unwrap();
    let c2c = types::kms::NodeIdentityStatusPayload::decode(c2c.payload().unwrap()).unwrap();
    assert_eq!(
        c2c.state,
        types::kms::NodeIdentityState::ProviderUnavailable
    );
    assert_eq!(CALLS.load(Ordering::Relaxed), 0);

    let relay = service
        .handle(
            &request(KmsOpcode::GetRelayP256Status, &[]),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(7)),
        )
        .unwrap();
    let relay = RelayP256StatusPayload::decode(relay.payload().unwrap()).unwrap();
    assert_eq!(relay.readiness, KmsCapabilityReadiness::Ready);
    assert_eq!(CALLS.load(Ordering::Relaxed), 1);

    let mut unavailable = KmsService::new();
    bind_net(&mut unavailable, 7, 4);
    let denied = unavailable
        .handle(
            &request(KmsOpcode::GetRelayP256Status, &[]),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(7)),
        )
        .unwrap();
    assert_error(denied, KmsErrorCode::RelayUnavailable);
}

#[test]
fn profile_generation_and_qualification_mismatches_fail_closed() {
    let mut service = KmsService::with_provider_fixture(FixtureRelayProvider::production());
    bind_net(&mut service, 7, 4);
    for (frame, expected) in [
        (
            sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION + 1, 99),
            KmsErrorCode::RelayGenerationMismatch,
        ),
        (
            sign_request([0x24; 32], FIXTURE_RELAY_GENERATION, 99),
            KmsErrorCode::ActiveProfileMismatch,
        ),
    ] {
        let response = service
            .handle(&frame, 7, Some(caller(50, 4, 7)), net_registry(Some(7)))
            .unwrap();
        assert_error(response, expected);
    }

    let provider = FixtureRelayProvider {
        assessment: RelayProviderAssessment::QualificationTest,
        ..FixtureRelayProvider::production()
    };
    let mut service = KmsService::with_provider_fixture(provider);
    bind_net(&mut service, 7, 4);
    let response = service
        .handle(
            &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(7)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::QualificationRequired);
}

#[test]
fn authenticated_time_unavailable_or_regressed_refuses_signing() {
    for floor in [0, 1_699_999_999] {
        let provider = FixtureRelayProvider::production();
        provider.authenticated_time_floor.set(floor);
        let mut service = KmsService::with_provider_fixture(provider);
        bind_net(&mut service, 7, 4);
        let response = service
            .handle(
                &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION, 99),
                7,
                Some(caller(50, 4, 7)),
                net_registry(Some(7)),
            )
            .unwrap();
        assert_error(response, KmsErrorCode::TimeUntrusted);
    }
}

#[test]
fn retired_generation_unavailable_precedes_untrusted_time() {
    let provider = FixtureRelayProvider::production();
    provider.authenticated_time_floor.set(0);
    let mut service = KmsService::with_provider_fixture(provider);
    bind_net(&mut service, 7, 4);
    let response = service
        .handle(
            &sign_request(FIXTURE_PROFILE_DIGEST, FIXTURE_RELAY_GENERATION - 1, 99),
            7,
            Some(caller(50, 4, 7)),
            net_registry(Some(7)),
        )
        .unwrap();
    assert_error(response, KmsErrorCode::RelayUnavailable);
}
